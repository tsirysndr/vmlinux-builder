// Deno native test runner
// Run with: deno test --allow-read deno-test.ts

import { assert, assertEquals, assertExists } from "@std/assert";
import {
  KernelConfigDeserializer,
  KernelConfigParser,
  KernelConfigSerializer,
  type SerializeOptions,
} from "./config.ts";

// Test data
const simpleConfig = `
CONFIG_64BIT=y
CONFIG_SMP=y
CONFIG_NR_CPUS=64
# CONFIG_DEBUG is not set
CONFIG_LOCALVERSION="-test"
`;

const complexConfig = `
#
# Automatically generated file; DO NOT EDIT.
# Linux/x86 6.6.100 Kernel Configuration
#
CONFIG_CC_VERSION_TEXT="gcc 13.3.0"
CONFIG_64BIT=y

#
# General setup
#
CONFIG_SMP=y
CONFIG_NR_CPUS=64
# CONFIG_EXPERT is not set
# end of General setup

#
# Security
#
CONFIG_SECURITY=y
CONFIG_SECURITY_SELINUX=y
# end of Security
`;

// ============================================================================
// DESERIALIZATION TESTS
// ============================================================================

Deno.test("deserialize simple config", () => {
  const config = KernelConfigDeserializer.deserialize(simpleConfig);
  assertEquals(Object.keys(config.flatConfig).length, 5);
});

Deno.test("deserialize with sections", () => {
  const config = KernelConfigDeserializer.deserialize(complexConfig, {
    parseSections: true,
  });
  assert(config.sections.length > 0);
});

Deno.test("parse boolean values", () => {
  const config = KernelConfigDeserializer.deserialize(simpleConfig);
  assertEquals(config.flatConfig.CONFIG_64BIT, "y");
  assertEquals(config.flatConfig.CONFIG_SMP, "y");
});

Deno.test("parse numeric values", () => {
  const config = KernelConfigDeserializer.deserialize(simpleConfig);
  assertEquals(config.flatConfig.CONFIG_NR_CPUS, 64);
});

Deno.test("parse string values", () => {
  const config = KernelConfigDeserializer.deserialize(simpleConfig);
  assertEquals(config.flatConfig.CONFIG_LOCALVERSION, "-test");
});

Deno.test("parse disabled options", () => {
  const config = KernelConfigDeserializer.deserialize(simpleConfig);
  assertEquals(config.flatConfig.CONFIG_DEBUG, undefined);
});

Deno.test("parse hex values", () => {
  const hexConfig = "CONFIG_PHYSICAL_START=0x1000000";
  const config = KernelConfigDeserializer.deserialize(hexConfig);
  assertEquals(config.flatConfig.CONFIG_PHYSICAL_START, 0x1000000);
});

Deno.test("deserialize from JSON", () => {
  const json = '{"sections":[],"flatConfig":{"CONFIG_SMP":"y"}}';
  const config = KernelConfigDeserializer.fromJSON(json);
  assertEquals(config.flatConfig.CONFIG_SMP, "y");
});

Deno.test("deserialize from object", () => {
  const obj = { CONFIG_SMP: true, CONFIG_DEBUG: false };
  const config = KernelConfigDeserializer.fromObject(obj);
  assertEquals(config.flatConfig.CONFIG_SMP, "y");
  assertEquals(config.flatConfig.CONFIG_DEBUG, undefined);
});

Deno.test("handle empty config", () => {
  const config = KernelConfigDeserializer.deserialize("");
  assertEquals(Object.keys(config.flatConfig).length, 0);
});

// ============================================================================
// SERIALIZATION TESTS
// ============================================================================

Deno.test("serialize to config format", () => {
  const config = KernelConfigDeserializer.deserialize(simpleConfig);
  const serialized = KernelConfigSerializer.toConfig(config);
  assert(serialized.includes("CONFIG_64BIT=y"));
});

Deno.test("serialize with custom options", () => {
  const config = KernelConfigDeserializer.deserialize(simpleConfig);
  const opts: SerializeOptions = { addHeader: false };
  const serialized = KernelConfigSerializer.toConfig(config, opts);
  assert(!serialized.startsWith("#"));
});

Deno.test("serialize sorted keys", () => {
  const config = KernelConfigDeserializer.deserialize(simpleConfig);
  const opts: SerializeOptions = { sortKeys: true };
  const serialized = KernelConfigSerializer.toConfig(config, opts);
  const lines = serialized.split("\n").filter((l) => l.startsWith("CONFIG_"));

  // Check if sorted
  for (let i = 1; i < lines.length; i++) {
    assert(lines[i - 1] <= lines[i]);
  }
});

Deno.test("serialize to JSON", () => {
  const config = KernelConfigDeserializer.deserialize(simpleConfig);
  const json = KernelConfigSerializer.toJSON(config);
  const parsed = JSON.parse(json);
  assertEquals(parsed.flatConfig.CONFIG_SMP, "y");
});

Deno.test("serialize to object", () => {
  const config = KernelConfigDeserializer.deserialize(simpleConfig);
  const obj = KernelConfigSerializer.toObject(config);
  assertEquals(obj.CONFIG_64BIT, "y");
});

Deno.test("serialize to object with boolean style", () => {
  const config = KernelConfigDeserializer.deserialize(simpleConfig);
  const obj = KernelConfigSerializer.toObject(config, true);
  assertEquals(obj.CONFIG_64BIT, true);
  assertEquals(obj.CONFIG_DEBUG, false);
});

Deno.test("serialize to YAML", () => {
  const config = KernelConfigDeserializer.deserialize(simpleConfig);
  const yaml = KernelConfigSerializer.toYAML(config);
  assert(yaml.includes("config:"));
  assert(yaml.includes('CONFIG_SMP: "y"'));
});

Deno.test("serialize to Makefile", () => {
  const config = KernelConfigDeserializer.deserialize(simpleConfig);
  const makefile = KernelConfigSerializer.toMakefile(config);
  assert(makefile.includes("SMP := y"));
});

Deno.test("serialize enabled only", () => {
  const config = KernelConfigDeserializer.deserialize(simpleConfig);
  const enabled = KernelConfigSerializer.toEnabledOnly(config);
  assert(enabled.includes("CONFIG_SMP=y"));
  assert(!enabled.includes("CONFIG_DEBUG"));
});

Deno.test("serialize to shell script", () => {
  const config = KernelConfigDeserializer.deserialize(simpleConfig);
  const shell = KernelConfigSerializer.toShellScript(config);
  assert(shell.includes("#!/bin/bash"));
  assert(shell.includes("export CONFIG_SMP=y"));
});

// ============================================================================
// ROUND-TRIP TESTS
// ============================================================================

Deno.test("round-trip simple config", () => {
  const original = KernelConfigDeserializer.deserialize(simpleConfig);
  const serialized = KernelConfigSerializer.toConfig(original);
  const roundtrip = KernelConfigDeserializer.deserialize(serialized);
  assertEquals(
    JSON.stringify(original.flatConfig),
    JSON.stringify(roundtrip.flatConfig)
  );
});

Deno.test("round-trip complex config", () => {
  const original = KernelConfigDeserializer.deserialize(complexConfig);
  const serialized = KernelConfigSerializer.toConfig(original);
  const roundtrip = KernelConfigDeserializer.deserialize(serialized);
  assertEquals(
    Object.keys(original.flatConfig).length,
    Object.keys(roundtrip.flatConfig).length
  );
});

Deno.test("round-trip via JSON", () => {
  const original = KernelConfigDeserializer.deserialize(simpleConfig);
  const json = KernelConfigSerializer.toJSON(original);
  const roundtrip = KernelConfigDeserializer.fromJSON(json);
  assertEquals(
    JSON.stringify(original.flatConfig),
    JSON.stringify(roundtrip.flatConfig)
  );
});

Deno.test("round-trip via object", () => {
  const original = KernelConfigDeserializer.fromObject({
    CONFIG_SMP: true,
    CONFIG_DEBUG: false,
  });
  const obj = KernelConfigSerializer.toObject(original, true);
  const roundtrip = KernelConfigDeserializer.fromObject(obj);
  assertEquals(original.flatConfig.CONFIG_SMP, roundtrip.flatConfig.CONFIG_SMP);
});

// ============================================================================
// DIFF TESTS
// ============================================================================

Deno.test("detect added options", () => {
  const old = KernelConfigDeserializer.fromObject({ CONFIG_SMP: true });
  const new_ = KernelConfigDeserializer.fromObject({
    CONFIG_SMP: true,
    CONFIG_DEBUG: true,
  });
  const diff = KernelConfigSerializer.toDiff(old, new_);
  assert(diff.includes("+ CONFIG_DEBUG=y"));
});

Deno.test("detect removed options", () => {
  const old = KernelConfigDeserializer.fromObject({
    CONFIG_SMP: true,
    CONFIG_DEBUG: true,
  });
  const new_ = KernelConfigDeserializer.fromObject({ CONFIG_SMP: true });
  const diff = KernelConfigSerializer.toDiff(old, new_);
  assert(diff.includes("- CONFIG_DEBUG=y"));
});

Deno.test("detect changed options", () => {
  const old = KernelConfigDeserializer.fromObject({ CONFIG_NR_CPUS: 64 });
  const new_ = KernelConfigDeserializer.fromObject({ CONFIG_NR_CPUS: 128 });
  const diff = KernelConfigSerializer.toDiff(old, new_);
  assert(diff.includes("- CONFIG_NR_CPUS=64"));
  assert(diff.includes("+ CONFIG_NR_CPUS=128"));
});

// ============================================================================
// EDGE CASES
// ============================================================================

Deno.test("handle quoted strings with spaces", () => {
  const config = KernelConfigDeserializer.deserialize(
    'CONFIG_TEXT="hello world"'
  );
  assertEquals(config.flatConfig.CONFIG_TEXT, "hello world");
});

Deno.test("handle quoted strings with special chars", () => {
  const config = KernelConfigDeserializer.deserialize(
    'CONFIG_TEXT="test#value"'
  );
  assertEquals(config.flatConfig.CONFIG_TEXT, "test#value");
});

Deno.test("handle empty string value", () => {
  const config = KernelConfigDeserializer.deserialize('CONFIG_TEXT=""');
  assertEquals(config.flatConfig.CONFIG_TEXT, "");
});

Deno.test("handle zero value", () => {
  const config = KernelConfigDeserializer.deserialize("CONFIG_NUM=0");
  assertEquals(config.flatConfig.CONFIG_NUM, 0);
});

Deno.test("handle negative numbers", () => {
  const config = KernelConfigDeserializer.deserialize("CONFIG_NUM=-1");
  assertEquals(config.flatConfig.CONFIG_NUM, -1);
});

Deno.test("handle module value (m)", () => {
  const config = KernelConfigDeserializer.deserialize("CONFIG_MODULE=m");
  assertEquals(config.flatConfig.CONFIG_MODULE, "m");
});

Deno.test("preserve section hierarchy", () => {
  const config = KernelConfigDeserializer.deserialize(complexConfig);
  const serialized = KernelConfigSerializer.toConfig(config, {
    preserveSections: true,
  });
  assert(serialized.includes("# General setup"));
  assert(serialized.includes("# Security"));
});

Deno.test("flatten sections when disabled", () => {
  const config = KernelConfigDeserializer.deserialize(complexConfig);
  const serialized = KernelConfigSerializer.toConfig(config, {
    preserveSections: false,
  });
  assert(!serialized.includes("# end of"));
});

Deno.test("handle malformed input gracefully", () => {
  const config = KernelConfigDeserializer.deserialize("INVALID LINE\n###\n", {
    strict: false,
  });
  assertExists(config);
});

// ============================================================================
// PARSER UTILITY TESTS
// ============================================================================

Deno.test("getValue returns correct value", () => {
  const config = KernelConfigDeserializer.deserialize(simpleConfig);
  const value = KernelConfigParser.getValue(config, "CONFIG_SMP");
  assertEquals(value, "y");
});

Deno.test("getValue returns null for non-existent key", () => {
  const config = KernelConfigDeserializer.deserialize(simpleConfig);
  const value = KernelConfigParser.getValue(config, "CONFIG_NONEXISTENT");
  assertEquals(value, undefined);
});

Deno.test("isEnabled detects enabled options", () => {
  const config = KernelConfigDeserializer.deserialize(simpleConfig);
  assert(KernelConfigParser.isEnabled(config, "CONFIG_SMP"));
});

Deno.test("isEnabled detects disabled options", () => {
  const config = KernelConfigDeserializer.deserialize(simpleConfig);
  assert(!KernelConfigParser.isEnabled(config, "CONFIG_DEBUG"));
});

Deno.test("extract processor config", () => {
  const config = KernelConfigDeserializer.deserialize(simpleConfig);
  const processor = KernelConfigParser.extractProcessorConfig(config);
  assertEquals(processor.SMP, true);
  assertEquals(processor.NR_CPUS, 64);
});

Deno.test("extract security config", () => {
  const config = KernelConfigDeserializer.deserialize(complexConfig);
  const security = KernelConfigParser.extractSecurityConfig(config);
  assertEquals(security.SECURITY, true);
  assertEquals(security.SECURITY_SELINUX, true);
});
