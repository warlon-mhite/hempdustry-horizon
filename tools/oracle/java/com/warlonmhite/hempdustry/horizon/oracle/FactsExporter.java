/*
 * Ported to Fabric and Minecraft 26.3 from Horizon's NeoForge oracle
 * (mods/horizon/src/neoforge-oracle/java/dev/horizonlab/neoforge/FactsExporter.java),
 * Copyright (c) 2026 Ludovic Marie and the Horizon contributors, MIT License:
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
 * associated documentation files (the "Software"), to deal in the Software without restriction,
 * including without limitation the rights to use, copy, modify, merge, publish, distribute,
 * sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions: The above copyright notice and this
 * permission notice shall be included in all copies or substantial portions of the Software.
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED.
 */
package com.warlonmhite.hempdustry.horizon.oracle;

import com.google.gson.JsonElement;
import com.google.gson.stream.JsonWriter;
import com.mojang.serialization.DynamicOps;
import com.mojang.serialization.JsonOps;
import java.io.IOException;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;
import net.fabricmc.api.ModInitializer;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerLifecycleEvents;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.component.TypedDataComponent;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.item.BlockItem;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.item.context.BlockPlaceContext;
import net.minecraft.world.item.enchantment.Enchantments;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.SoundType;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.Property;
import net.minecraft.world.level.storage.loot.LootParams;
import net.minecraft.world.level.storage.loot.parameters.LootContextParams;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.BlockHitResult;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.phys.shapes.VoxelShape;

/**
 * Dev only, never shipped: once the server has started, writes every block, item and tag of the
 * namespaces in {@code horizon.oracle.namespaces} (and vanilla's blocks, for templates) to
 * {@code horizon.oracle.out}, in the shape Horizon's port tool reads as an oracle, then stops.
 */
public final class FactsExporter implements ModInitializer {
    private static final BlockPos FLOOR = new BlockPos(0, 99, 0);

    @Override
    public void onInitialize() {
        ServerLifecycleEvents.SERVER_STARTED.register(FactsExporter::started);
    }

    private static void started(MinecraftServer server) {
        Path out = Path.of(System.getProperty("horizon.oracle.out", "horizon-oracle.json"));
        Set<String> namespaces =
                new TreeSet<>(Arrays.asList(System.getProperty("horizon.oracle.namespaces", "").split(",")));
        namespaces.remove("");
        try (Writer w = Files.newBufferedWriter(out, StandardCharsets.UTF_8);
                JsonWriter json = new JsonWriter(w)) {
            new Export(server, namespaces, json).run();
        } catch (Throwable e) {
            e.printStackTrace();
            System.exit(3);
        }
        System.out.println("horizon-oracle: wrote " + out.toAbsolutePath());
        server.halt(false);
    }

    private static final class Export {
        final MinecraftServer server;
        final ServerLevel level;
        final Set<String> namespaces;
        final JsonWriter json;
        final List<ItemStack> tools = new ArrayList<>();
        final List<String> toolNames = new ArrayList<>();

        Export(MinecraftServer server, Set<String> namespaces, JsonWriter json) {
            this.server = server;
            this.level = server.overworld();
            this.namespaces = namespaces;
            this.json = json;
            tool("hand", ItemStack.EMPTY);
            tool("diamond_pickaxe", new ItemStack(Items.DIAMOND_PICKAXE));
            tool("diamond_axe", new ItemStack(Items.DIAMOND_AXE));
            tool("diamond_shovel", new ItemStack(Items.DIAMOND_SHOVEL));
            tool("diamond_hoe", new ItemStack(Items.DIAMOND_HOE));
            tool("shears", new ItemStack(Items.SHEARS));
            ItemStack silk = new ItemStack(Items.DIAMOND_PICKAXE);
            silk.enchant(server.registryAccess().lookupOrThrow(Registries.ENCHANTMENT)
                    .getOrThrow(Enchantments.SILK_TOUCH), 1);
            tool("silk_touch_pickaxe", silk);
        }

        void tool(String name, ItemStack stack) {
            toolNames.add(name);
            tools.add(stack);
        }

        boolean ours(Identifier id) {
            return namespaces.contains(id.getNamespace());
        }

        void run() throws IOException {
            json.beginObject();
            json.name("schema").value(1);
            json.name("minecraft").value(SharedConstants.getCurrentVersion().name());
            json.name("namespaces");
            json.beginArray();
            for (String n : namespaces) json.value(n);
            json.endArray();
            json.name("tools");
            json.beginArray();
            for (String n : toolNames) json.value(n);
            json.endArray();
            blocks();
            json.name("vanilla_items");
            json.beginArray();
            for (Item item : BuiltInRegistries.ITEM) {
                Identifier id = BuiltInRegistries.ITEM.getKey(item);
                if (id.getNamespace().equals("minecraft")) json.value(id.toString());
            }
            json.endArray();
            items();
            creativeTabs();
            tags();
            json.endObject();
        }

        void blocks() throws IOException {
            level.setBlock(FLOOR, Blocks.STONE.defaultBlockState(), 3);
            json.name("blocks");
            json.beginObject();
            for (Block block : BuiltInRegistries.BLOCK) {
                Identifier id = BuiltInRegistries.BLOCK.getKey(block);
                boolean mod = ours(id);
                if (!mod && !id.getNamespace().equals("minecraft")) continue;
                json.name(id.toString());
                json.beginObject();
                json.name("class");
                json.beginArray();
                for (Class<?> c = block.getClass(); c != null && c != Object.class; c = c.getSuperclass()) {
                    json.value(c.getName());
                }
                json.endArray();
                BlockState def = block.defaultBlockState();
                json.name("properties");
                json.beginObject();
                for (Property<?> p : block.getStateDefinition().getProperties()) {
                    json.name(p.getName());
                    json.beginArray();
                    for (Object v : p.getPossibleValues()) json.value(name(p, v));
                    json.endArray();
                }
                json.endObject();
                json.name("default").value(stateKey(def));
                json.name("item").value(BuiltInRegistries.ITEM.getKey(block.asItem()).toString());
                json.name("explosion_resistance").value(block.getExplosionResistance());
                json.name("friction").value(block.getFriction());
                json.name("speed_factor").value(block.getSpeedFactor());
                json.name("jump_factor").value(block.getJumpFactor());
                blockSetType(block);
                json.name("states");
                json.beginObject();
                for (BlockState state : block.getStateDefinition().getPossibleStates()) {
                    json.name(stateKey(state));
                    json.beginObject();
                    json.name("hardness").value(state.getDestroySpeed(EmptyBlockGetter.INSTANCE, BlockPos.ZERO));
                    json.name("emission").value(state.getLightEmission());
                    json.name("requires_tool").value(state.requiresCorrectToolForDrops());
                    json.name("can_occlude").value(state.canOcclude());
                    json.name("replaceable").value(state.canBeReplaced());
                    json.name("random_ticks").value(state.isRandomlyTicking());
                    var color = state.getMapColor(EmptyBlockGetter.INSTANCE, BlockPos.ZERO);
                    json.name("map_color").value(color.col);
                    json.name("map_color_id").value(color.id);
                    sound(state.getSoundType());
                    json.name("outline");
                    boxes(state.getShape(EmptyBlockGetter.INSTANCE, BlockPos.ZERO));
                    json.name("collision");
                    boxes(state.getCollisionShape(EmptyBlockGetter.INSTANCE, BlockPos.ZERO));
                    if (mod && lootSample(block, state)) drops(state);
                    json.endObject();
                }
                json.endObject();
                if (block.asItem() instanceof BlockItem bi) placement(bi);
                json.endObject();
            }
            json.endObject();
        }

        /** Every state of a small block; else the default and each one-property change from it. */
        boolean lootSample(Block block, BlockState state) {
            if (block.getStateDefinition().getPossibleStates().size() <= 32) return true;
            BlockState def = block.defaultBlockState();
            int differ = 0;
            for (Property<?> p : block.getStateDefinition().getProperties()) {
                if (!state.getValue(p).equals(def.getValue(p))) differ++;
            }
            return differ <= 1;
        }

        void drops(BlockState state) throws IOException {
            json.name("drops");
            json.beginObject();
            for (int i = 0; i < tools.size(); i++) {
                LootParams.Builder params = new LootParams.Builder(level)
                        .withParameter(LootContextParams.ORIGIN, Vec3.atCenterOf(FLOOR.above()))
                        .withParameter(LootContextParams.TOOL, tools.get(i).copy());
                Map<String, Integer> counts = new TreeMap<>();
                try {
                    for (ItemStack s : state.getDrops(params)) {
                        counts.merge(BuiltInRegistries.ITEM.getKey(s.getItem()).toString(), s.getCount(), Integer::sum);
                    }
                } catch (RuntimeException e) {
                    // A table that needs what a bare roll lacks (an entity, a block entity): no drops recorded.
                }
                json.name(toolNames.get(i));
                json.beginObject();
                for (Map.Entry<String, Integer> e : counts.entrySet()) json.name(e.getKey()).value(e.getValue());
                json.endObject();
            }
            json.endObject();
        }

        void placement(BlockItem item) throws IOException {
            BlockPos at = FLOOR.above();
            level.setBlock(at, Blocks.AIR.defaultBlockState(), 3);
            json.name("placement");
            json.beginObject();
            for (Direction face : new Direction[] {Direction.UP, Direction.NORTH}) {
                BlockPos clicked = face == Direction.UP ? FLOOR : at.relative(face.getOpposite());
                level.setBlock(clicked, Blocks.STONE.defaultBlockState(), 3);
                Vec3 hit = Vec3.atCenterOf(clicked).add(Vec3.atLowerCornerOf(face.getUnitVec3i()).scale(0.5));
                BlockPlaceContext ctx = new BlockPlaceContext(level, null, InteractionHand.MAIN_HAND,
                        new ItemStack(item), new BlockHitResult(hit, face, clicked, false)) {};
                BlockState placed = null;
                try {
                    placed = item.getBlock().getStateForPlacement(ctx);
                } catch (RuntimeException e) {
                    // Needs a player: recorded as no placement.
                }
                json.name(face.getName()).value(placed == null ? null : stateKey(placed));
                if (face != Direction.UP) level.setBlock(clicked, Blocks.AIR.defaultBlockState(), 3);
            }
            json.endObject();
        }

        /** The BlockSetType (doors, trapdoors, buttons, plates) and WoodType (gates, signs) a block keeps. */
        void blockSetType(Block block) throws IOException {
            String setType = null, woodType = null;
            Boolean byHand = null;
            for (Class<?> c = block.getClass(); c != null && c != Object.class; c = c.getSuperclass()) {
                for (var f : c.getDeclaredFields()) {
                    try {
                        if (f.getType() == net.minecraft.world.level.block.state.properties.BlockSetType.class && setType == null) {
                            f.setAccessible(true);
                            var t = (net.minecraft.world.level.block.state.properties.BlockSetType) f.get(block);
                            if (t != null) {
                                setType = t.name();
                                byHand = t.canOpenByHand();
                            }
                        } else if (f.getType() == net.minecraft.world.level.block.state.properties.WoodType.class && woodType == null) {
                            f.setAccessible(true);
                            var t = (net.minecraft.world.level.block.state.properties.WoodType) f.get(block);
                            if (t != null) woodType = t.name();
                        }
                    } catch (ReflectiveOperationException | RuntimeException e) {
                        // Not readable: left out.
                    }
                }
            }
            if (setType != null) {
                json.name("block_set_type");
                json.beginObject();
                json.name("name").value(setType);
                json.name("can_open_by_hand").value(byHand);
                json.endObject();
            }
            if (woodType != null) json.name("wood_type").value(woodType);
        }

        void sound(SoundType s) throws IOException {
            json.name("sound");
            json.beginObject();
            json.name("volume").value(s.getVolume());
            json.name("pitch").value(s.getPitch());
            event("break", s.getBreakSound());
            event("step", s.getStepSound());
            event("place", s.getPlaceSound());
            event("hit", s.getHitSound());
            event("fall", s.getFallSound());
            json.endObject();
        }

        void event(String name, SoundEvent e) throws IOException {
            json.name(name).value(e.location().toString());
        }

        void boxes(VoxelShape shape) throws IOException {
            json.beginArray();
            for (AABB b : shape.toAabbs()) {
                json.beginArray();
                json.value(b.minX).value(b.minY).value(b.minZ).value(b.maxX).value(b.maxY).value(b.maxZ);
                json.endArray();
            }
            json.endArray();
        }

        void items() throws IOException {
            json.name("items");
            json.beginObject();
            DynamicOps<JsonElement> ops = server.registryAccess().createSerializationContext(JsonOps.INSTANCE);
            for (Item item : BuiltInRegistries.ITEM) {
                Identifier id = BuiltInRegistries.ITEM.getKey(item);
                if (!ours(id)) continue;
                ItemStack stack = new ItemStack(item);
                json.name(id.toString());
                json.beginObject();
                json.name("class").value(item.getClass().getName());
                json.name("max_stack_size").value(stack.getMaxStackSize());
                json.name("rarity").value(stack.getRarity().getSerializedName());
                if (item instanceof BlockItem bi) {
                    json.name("block").value(BuiltInRegistries.BLOCK.getKey(bi.getBlock()).toString());
                }
                // What a recipe leaves in the grid: Item.Properties.craftRemainder, not a component.
                var remainder = item.getCraftingRemainder();
                if (remainder != null) {
                    json.name("crafting_remainder")
                            .value(BuiltInRegistries.ITEM.getKey(remainder.item().value()).toString());
                }
                var food = stack.get(net.minecraft.core.component.DataComponents.FOOD);
                if (food != null) {
                    json.name("food");
                    json.beginObject();
                    json.name("nutrition").value(food.nutrition());
                    json.name("saturation").value(food.saturation());
                    json.endObject();
                }
                // 26.3's own default components: armour, repair and enchantability are components here.
                json.name("components");
                json.beginObject();
                for (TypedDataComponent<?> c : stack.getComponents()) {
                    var encoded = c.encodeValue(ops).result();
                    if (encoded.isPresent()) {
                        json.name(BuiltInRegistries.DATA_COMPONENT_TYPE.getKey(c.type()).toString());
                        json.jsonValue(encoded.get().toString());
                    }
                }
                json.endObject();
                json.endObject();
            }
            json.endObject();
        }

        /** Our creative tabs: title key, icon and item ids in order (each once: Horizon lists items, not stacks). */
        void creativeTabs() throws IOException {
            net.minecraft.world.item.CreativeModeTabs.tryRebuildTabContents(
                    server.getWorldData().enabledFeatures(), false, server.registryAccess());
            json.name("creative_tabs");
            json.beginObject();
            for (var tab : BuiltInRegistries.CREATIVE_MODE_TAB) {
                Identifier id = BuiltInRegistries.CREATIVE_MODE_TAB.getKey(tab);
                if (!ours(id)) continue;
                json.name(id.toString());
                json.beginObject();
                var title = tab.getDisplayName().getContents();
                json.name("title").value(title instanceof net.minecraft.network.chat.contents.TranslatableContents t
                        ? t.getKey() : tab.getDisplayName().getString());
                json.name("icon").value(BuiltInRegistries.ITEM.getKey(tab.getIconItem().getItem()).toString());
                json.name("items");
                json.beginArray();
                Set<String> seen = new LinkedHashSet<>();
                for (ItemStack s : tab.getDisplayItems()) seen.add(BuiltInRegistries.ITEM.getKey(s.getItem()).toString());
                for (String s : seen) json.value(s);
                json.endArray();
                json.endObject();
            }
            json.endObject();
        }

        /** Tags that hold one of our ids, with only those members; and every `c` tag, whole. */
        void tags() throws IOException {
            json.name("tags");
            json.beginObject();
            for (String kind : new String[] {"block", "item"}) {
                json.name(kind);
                json.beginObject();
                Map<String, Set<String>> out = new TreeMap<>();
                var registry = kind.equals("block") ? BuiltInRegistries.BLOCK : BuiltInRegistries.ITEM;
                registry.getTags().forEach(named -> {
                    Identifier tagId = named.key().location();
                    boolean convention = tagId.getNamespace().equals("c");
                    Set<String> members = new LinkedHashSet<>();
                    for (var holder : named) {
                        Identifier id = holder.unwrapKey().orElseThrow().identifier();
                        if (convention || ours(id) || ours(tagId)) members.add(id.toString());
                    }
                    if (!members.isEmpty()) out.put(tagId.toString(), new TreeSet<>(members));
                });
                for (var e : out.entrySet()) {
                    json.name(e.getKey());
                    json.beginArray();
                    for (String m : e.getValue()) json.value(m);
                    json.endArray();
                }
                json.endObject();
            }
            json.endObject();
        }

        static String stateKey(BlockState state) {
            StringBuilder b = new StringBuilder();
            for (Property<?> p : state.getProperties()) {
                if (b.length() > 0) b.append(',');
                b.append(p.getName()).append('=').append(name(p, state.getValue(p)));
            }
            return b.toString();
        }

        @SuppressWarnings({"unchecked", "rawtypes"})
        static String name(Property p, Object v) {
            return p.getName((Comparable) v);
        }
    }
}
