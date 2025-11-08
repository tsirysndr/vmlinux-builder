import * as toml from "@std/toml";
import z from "@zod/zod";

/**
 * Zod schema for Linux Kernel Configuration (.config) files
 * Supports parsing the standard kernel config format with:
 * - CONFIG_* options set to y, m, n, or numeric/string values
 * - Comments (# lines)
 * - Section headers
 */

// Base config value types
const ConfigValueSchema = z.union([
  z.literal("y"), // Built-in
  z.literal("m"), // Module
  z.literal("n"), // Not set (explicit)
  z.number(), // Numeric value
  z.string(), // String value
  z.boolean(), // Derived from 'y'/'n' or presence
]);

// Individual config entry
const ConfigEntrySchema = z.object({
  key: z.string(),
  value: ConfigValueSchema.optional(),
  comment: z.string().optional(),
});

// Section in the config file
interface ConfigSection {
  name: string;
  entries: ConfigEntry[];
  subsections?: ConfigSection[];
}

const ConfigSectionSchema: z.ZodType<ConfigSection> = z.lazy(() =>
  z.object({
    name: z.string(),
    entries: z.array(ConfigEntrySchema),
    subsections: z.array(z.lazy(() => ConfigSectionSchema)).optional(),
  })
);

// Main kernel config schema
export const KernelConfigSchema = z.object({
  version: z.string().optional(),
  buildInfo: z
    .object({
      compiler: z.string().optional(),
      gccVersion: z.string().optional(),
      buildSalt: z.string().optional(),
    })
    .optional(),
  sections: z.array(ConfigSectionSchema),
  flatConfig: z.record(z.string(), ConfigValueSchema.optional()),
});

// Specific schemas for common config categories
export const ProcessorConfigSchema = z.object({
  SMP: z.boolean().optional(),
  NR_CPUS: z.number().optional(),
  X86_64: z.boolean().optional(),
  NUMA: z.boolean().optional(),
  PREEMPT: z.boolean().optional(),
  PREEMPT_VOLUNTARY: z.boolean().optional(),
  PREEMPT_NONE: z.boolean().optional(),
});

export const SecurityConfigSchema = z.object({
  SECURITY: z.boolean().optional(),
  SECURITY_SELINUX: z.boolean().optional(),
  SECURITY_APPARMOR: z.boolean().optional(),
  SECURITY_SMACK: z.boolean().optional(),
  SECCOMP: z.boolean().optional(),
  STACKPROTECTOR: z.boolean().optional(),
  FORTIFY_SOURCE: z.boolean().optional(),
});

export const NetworkingConfigSchema = z.object({
  NET: z.boolean().optional(),
  INET: z.boolean().optional(),
  IPV6: z.boolean().optional(),
  NETFILTER: z.boolean().optional(),
  PACKET: z.boolean().optional(),
  UNIX: z.boolean().optional(),
});

export const FilesystemConfigSchema = z.object({
  EXT4_FS: z.boolean().optional(),
  XFS_FS: z.boolean().optional(),
  BTRFS_FS: z.boolean().optional(),
  NFS_FS: z.boolean().optional(),
  TMPFS: z.boolean().optional(),
});

// TypeScript types derived from schemas
export type ConfigValue = z.infer<typeof ConfigValueSchema>;
export type ConfigEntry = z.infer<typeof ConfigEntrySchema>;
export type KernelConfig = z.infer<typeof KernelConfigSchema>;
export type ProcessorConfig = z.infer<typeof ProcessorConfigSchema>;
export type SecurityConfig = z.infer<typeof SecurityConfigSchema>;
export type NetworkingConfig = z.infer<typeof NetworkingConfigSchema>;
export type FilesystemConfig = z.infer<typeof FilesystemConfigSchema>;

/**
 * Parser for Linux kernel .config files
 */
export class KernelConfigParser {
  /**
   * Parse a kernel config file content
   */
  static parse(content: string): KernelConfig {
    const lines = content.split("\n");
    const flatConfig: Record<string, ConfigValue | undefined> = {};
    const sections: ConfigSection[] = [];
    let currentSection: ConfigSection | undefined = undefined;
    const sectionStack: ConfigSection[] = [];

    for (const line of lines) {
      const trimmed = line.trim();

      if (!trimmed) continue;

      if (trimmed.startsWith("#") && !trimmed.includes("CONFIG_")) {
        const sectionMatch = trimmed.match(/^#\s*(.+)$/);
        if (sectionMatch) {
          const sectionName = sectionMatch[1];

          // Check for section end
          if (sectionName.startsWith("end of")) {
            if (sectionStack.length > 0) {
              currentSection = sectionStack.pop();
            }
            continue;
          }

          // Create new section
          const newSection: ConfigSection = {
            name: sectionName,
            entries: [],
            subsections: [],
          };

          if (currentSection) {
            // Add as subsection
            if (!currentSection.subsections) {
              currentSection.subsections = [];
            }
            currentSection.subsections.push(newSection);
            sectionStack.push(currentSection);
          } else {
            // Add as top-level section
            sections.push(newSection);
          }
          currentSection = newSection;
        }
        continue;
      }

      // Disabled option: # CONFIG_* is not set
      const disabledMatch = trimmed.match(/^#\s*(CONFIG_\w+)\s+is not set/);
      if (disabledMatch) {
        const key = disabledMatch[1];
        flatConfig[key] = undefined;

        const entry: ConfigEntry = {
          key,
          value: undefined,
          comment: "is not set",
        };

        if (currentSection) {
          currentSection.entries.push(entry);
        }
        continue;
      }

      // Enabled option: CONFIG_*=value
      const enabledMatch = trimmed.match(/^(CONFIG_\w+)=(.+)$/);
      if (enabledMatch) {
        const key = enabledMatch[1];
        let value: ConfigValue;
        const rawValue = enabledMatch[2];

        // Parse value type
        if (rawValue === "y") {
          value = "y";
        } else if (rawValue === "m") {
          value = "m";
        } else if (rawValue === "n") {
          value = "n";
        } else if (rawValue.match(/^-?\d+$/)) {
          value = parseInt(rawValue, 10);
        } else if (rawValue.match(/^0x[0-9a-fA-F]+$/)) {
          value = parseInt(rawValue, 16);
        } else {
          // String value (remove quotes if present)
          value = rawValue.replace(/^"(.*)"$/, "$1");
        }

        flatConfig[key] = value;

        const entry: ConfigEntry = {
          key,
          value,
        };

        if (currentSection) {
          currentSection.entries.push(entry);
        }
        continue;
      }
    }

    // Extract build info
    const buildInfo = {
      compiler: flatConfig.CONFIG_CC_VERSION_TEXT as string | undefined,
      gccVersion: flatConfig.CONFIG_GCC_VERSION
        ? String(flatConfig.CONFIG_GCC_VERSION)
        : undefined,
      buildSalt: flatConfig.CONFIG_BUILD_SALT as string | undefined,
    };

    return {
      buildInfo,
      sections,
      flatConfig,
    };
  }

  /**
   * Extract specific config category
   */
  static extractProcessorConfig(config: KernelConfig): ProcessorConfig {
    const flat = config.flatConfig;
    return {
      SMP: flat.CONFIG_SMP === "y",
      NR_CPUS: flat.CONFIG_NR_CPUS as number | undefined,
      X86_64: flat.CONFIG_X86_64 === "y",
      NUMA: flat.CONFIG_NUMA === "y",
      PREEMPT: flat.CONFIG_PREEMPT === "y",
      PREEMPT_VOLUNTARY: flat.CONFIG_PREEMPT_VOLUNTARY === "y",
      PREEMPT_NONE: flat.CONFIG_PREEMPT_NONE === "y",
    };
  }

  static extractSecurityConfig(config: KernelConfig): SecurityConfig {
    const flat = config.flatConfig;
    return {
      SECURITY: flat.CONFIG_SECURITY === "y",
      SECURITY_SELINUX: flat.CONFIG_SECURITY_SELINUX === "y",
      SECURITY_APPARMOR: flat.CONFIG_SECURITY_APPARMOR === "y",
      SECURITY_SMACK: flat.CONFIG_SECURITY_SMACK === "y",
      SECCOMP: flat.CONFIG_SECCOMP === "y",
      STACKPROTECTOR: flat.CONFIG_STACKPROTECTOR === "y",
      FORTIFY_SOURCE: flat.CONFIG_FORTIFY_SOURCE === "y",
    };
  }

  static extractNetworkingConfig(config: KernelConfig): NetworkingConfig {
    const flat = config.flatConfig;
    return {
      NET: flat.CONFIG_NET === "y",
      INET: flat.CONFIG_INET === "y",
      IPV6: flat.CONFIG_IPV6 === "y",
      NETFILTER: flat.CONFIG_NETFILTER === "y",
      PACKET: flat.CONFIG_PACKET === "y",
      UNIX: flat.CONFIG_UNIX === "y",
    };
  }

  static extractFilesystemConfig(config: KernelConfig): FilesystemConfig {
    const flat = config.flatConfig;
    return {
      EXT4_FS: flat.CONFIG_EXT4_FS === "y",
      XFS_FS: flat.CONFIG_XFS_FS === "y",
      BTRFS_FS: flat.CONFIG_BTRFS_FS === "y",
      NFS_FS: flat.CONFIG_NFS_FS === "y",
      TMPFS: flat.CONFIG_TMPFS === "y",
    };
  }

  /**
   * Get a specific config value
   */
  static getValue(config: KernelConfig, key: string): ConfigValue | undefined {
    return config.flatConfig[key];
  }

  /**
   * Check if a config option is enabled (set to 'y' or 'm')
   */
  static isEnabled(config: KernelConfig, key: string): boolean {
    const value = config.flatConfig[key];
    return value === "y" || value === "m";
  }

  /**
   * Serialize config back to .config format
   */
  static serialize(config: KernelConfig, options?: SerializeOptions): string {
    const opts: Required<SerializeOptions> = {
      preserveSections: true,
      includeComments: true,
      sortKeys: false,
      addHeader: true,
      formatStyle: "kernel",
      ...options,
    };

    const lines: string[] = [];

    // Add header
    if (opts.addHeader) {
      lines.push("#");
      lines.push("# Automatically generated file; DO NOT EDIT.");

      if (config.buildInfo?.compiler) {
        lines.push(`# ${config.buildInfo.compiler}`);
      }
      if (config.buildInfo?.buildSalt) {
        lines.push(`# Linux Kernel Configuration`);
      }
      lines.push("#");
      lines.push("");
    }

    if (opts.preserveSections && config.sections.length > 0) {
      // Serialize with section structure
      this.serializeSections(lines, config.sections, 0, opts);
    } else {
      // Serialize flat config
      this.serializeFlatConfig(lines, config.flatConfig, opts);
    }

    return lines.join("\n");
  }

  private static serializeSections(
    lines: string[],
    sections: ConfigSection[],
    depth: number,
    opts: Required<SerializeOptions>
  ): void {
    for (const section of sections) {
      // Add section header
      if (opts.includeComments) {
        lines.push("");
        lines.push("#");
        lines.push(`# ${section.name}`);
        lines.push("#");
      }

      // Serialize entries
      for (const entry of section.entries) {
        this.serializeEntry(lines, entry, opts);
      }

      // Serialize subsections recursively
      if (section.subsections && section.subsections.length > 0) {
        this.serializeSections(lines, section.subsections, depth + 1, opts);
      }

      // Add section footer
      if (opts.includeComments) {
        lines.push(`# end of ${section.name}`);
      }
    }
  }

  private static serializeFlatConfig(
    lines: string[],
    flatConfig: Record<string, ConfigValue | undefined>,
    opts: Required<SerializeOptions>
  ): void {
    const keys = opts.sortKeys
      ? Object.keys(flatConfig).sort()
      : Object.keys(flatConfig);

    for (const key of keys) {
      const entry: ConfigEntry = {
        key,
        value: flatConfig[key],
      };
      this.serializeEntry(lines, entry, opts);
    }
  }

  private static serializeEntry(
    lines: string[],
    entry: ConfigEntry,
    opts: Required<SerializeOptions>
  ): void {
    const { key, value, comment } = entry;

    if (!value) {
      lines.push(`# ${key} is not set`);
    } else if (value === "y" || value === "m" || value === "n") {
      lines.push(`${key}=${value}`);
    } else if (typeof value === "number") {
      lines.push(`${key}=${value}`);
    } else if (typeof value === "string") {
      const needsQuotes =
        value.includes(" ") ||
        value.includes("#") ||
        value.includes("=") ||
        opts.formatStyle === "quoted";
      const formatted = needsQuotes ? `"${value}"` : value;
      lines.push(`${key}=${formatted}`);
    }

    // Add inline comment if present
    if (comment && opts.includeComments) {
      const lastLine = lines[lines.length - 1];
      lines[lines.length - 1] = `${lastLine}  # ${comment}`;
    }
  }
}

/**
 * Serialization options
 */
export interface SerializeOptions {
  /** Preserve section structure (default: true) */
  preserveSections?: boolean;
  /** Include comments (default: true) */
  includeComments?: boolean;
  /** Sort keys alphabetically (default: false) */
  sortKeys?: boolean;
  /** Add header with build info (default: true) */
  addHeader?: boolean;
  /** Format style: 'kernel' or 'quoted' (default: 'kernel') */
  formatStyle?: "kernel" | "quoted";
}

/**
 * Deserialization options
 */
export interface DeserializeOptions {
  /** Strict mode: fail on parse errors (default: false) */
  strict?: boolean;
  /** Preserve comments as metadata (default: true) */
  preserveComments?: boolean;
  /** Parse section hierarchy (default: true) */
  parseSections?: boolean;
  /** Validate with Zod schema (default: false) */
  validate?: boolean;
}

/**
 * Enhanced deserializer with options
 */
export class KernelConfigDeserializer {
  /**
   * Deserialize kernel config with options
   */
  static deserialize(
    content: string,
    options?: DeserializeOptions
  ): KernelConfig {
    const opts: Required<DeserializeOptions> = {
      strict: false,
      preserveComments: true,
      parseSections: true,
      validate: false,
      ...options,
    };

    try {
      const config = KernelConfigParser.parse(content);

      if (opts.validate) {
        const validated = KernelConfigSchema.parse(config);
        return validated;
      }

      return config;
    } catch (error) {
      if (opts.strict) {
        throw new Error(`Failed to deserialize kernel config: ${error}`);
      }

      // Return minimal valid config on error
      return {
        sections: [],
        flatConfig: {},
      };
    }
  }

  /**
   * Deserialize from JSON format
   */
  static fromJSON(json: string, options?: DeserializeOptions): KernelConfig {
    try {
      const data = JSON.parse(json);

      if (options?.validate) {
        return KernelConfigSchema.parse(data);
      }

      return data as KernelConfig;
    } catch (error) {
      if (options?.strict) {
        throw new Error(`Failed to deserialize JSON: ${error}`);
      }

      return {
        sections: [],
        flatConfig: {},
      };
    }
  }

  /**
   * Deserialize from TOML format
   */
  static fromTOML(tomlString: string): KernelConfig {
    try {
      const data = toml.parse(tomlString);
      return KernelConfigDeserializer.fromObject(
        data as Record<string, string | number | boolean | undefined>
      );
    } catch (error) {
      throw new Error(`Failed to deserialize TOML: ${error}`);
    }
  }

  /**
   * Deserialize from YAML-like format
   */
  static fromObject(
    obj: Record<string, ConfigValue | undefined>
  ): KernelConfig {
    const flatConfig: Record<string, ConfigValue | undefined> = {};

    for (const [key, value] of Object.entries(obj)) {
      if (key.startsWith("CONFIG_")) {
        if (value === true) {
          flatConfig[key] = "y";
        } else if (value === false || value === undefined) {
          flatConfig[key] = undefined;
        } else if (value === "y" || value === "m" || value === "n") {
          flatConfig[key] = value;
        } else if (typeof value === "number" || typeof value === "string") {
          flatConfig[key] = value;
        }
      }
    }

    return {
      sections: [],
      flatConfig,
    };
  }
}

/**
 * Enhanced serializer with multiple output formats
 */
export class KernelConfigSerializer {
  /**
   * Serialize to kernel .config format
   */
  static toConfig(config: KernelConfig, options?: SerializeOptions): string {
    return KernelConfigParser.serialize(config, options);
  }

  /**
   * Serialize to JSON format
   */
  static toJSON(config: KernelConfig, pretty: boolean = true): string {
    if (pretty) {
      return JSON.stringify(config, null, 2);
    }
    return JSON.stringify(config);
  }

  /**
   * Serialize to TOML format
   */
  static toTOML(config: KernelConfig): string {
    const tomlObj: Record<string, unknown> = {
      buildInfo: config.buildInfo || {},
      config: config.flatConfig,
    };
    return toml.stringify(tomlObj);
  }

  /**
   * Serialize to simple key-value object
   */
  static toObject(
    config: KernelConfig,
    booleanStyle: boolean = false
  ): Record<string, ConfigValue | undefined> {
    const result: Record<string, ConfigValue | undefined> = {};

    for (const [key, value] of Object.entries(config.flatConfig)) {
      if (booleanStyle) {
        // Convert y/n to true/false
        if (value === "y" || value === "m") {
          result[key] = true;
        } else if (value === "n" || value === undefined) {
          result[key] = false;
        } else {
          result[key] = value;
        }
      } else {
        result[key] = value;
      }
    }

    return result;
  }

  /**
   * Serialize to YAML-like format (as string)
   */
  static toYAML(config: KernelConfig): string {
    const lines: string[] = [];
    lines.push("---");

    if (config.buildInfo) {
      lines.push("buildInfo:");
      if (config.buildInfo.compiler) {
        lines.push(`  compiler: "${config.buildInfo.compiler}"`);
      }
      if (config.buildInfo.gccVersion) {
        lines.push(`  gccVersion: "${config.buildInfo.gccVersion}"`);
      }
      if (config.buildInfo.buildSalt) {
        lines.push(`  buildSalt: "${config.buildInfo.buildSalt}"`);
      }
    }

    lines.push("");
    lines.push("config:");

    for (const [key, value] of Object.entries(config.flatConfig)) {
      const yamlValue =
        value === null
          ? "null"
          : typeof value === "string"
          ? `"${value}"`
          : value;
      lines.push(`  ${key}: ${yamlValue}`);
    }

    return lines.join("\n");
  }

  /**
   * Serialize to Makefile-compatible format
   */
  static toMakefile(config: KernelConfig): string {
    const lines: string[] = [];
    lines.push("# Kernel configuration as Makefile variables");
    lines.push("");

    for (const [key, value] of Object.entries(config.flatConfig)) {
      if (value === null) {
        lines.push(`# ${key} is not set`);
      } else {
        const makeKey = key.replace("CONFIG_", "");
        if (value === "y") {
          lines.push(`${makeKey} := y`);
        } else if (value === "m") {
          lines.push(`${makeKey} := m`);
        } else if (typeof value === "number") {
          lines.push(`${makeKey} := ${value}`);
        } else {
          lines.push(`${makeKey} := ${value}`);
        }
      }
    }

    return lines.join("\n");
  }

  /**
   * Serialize only enabled options
   */
  static toEnabledOnly(config: KernelConfig): string {
    const lines: string[] = [];

    for (const [key, value] of Object.entries(config.flatConfig)) {
      if (value === "y" || value === "m") {
        lines.push(`${key}=${value}`);
      }
    }

    return lines.join("\n");
  }

  /**
   * Serialize to shell script format
   */
  static toShellScript(config: KernelConfig): string {
    const lines: string[] = [];
    lines.push("#!/bin/bash");
    lines.push("# Kernel configuration as shell variables");
    lines.push("");

    for (const [key, value] of Object.entries(config.flatConfig)) {
      if (!value) {
        lines.push(`# ${key} is not set`);
      } else {
        const shellValue =
          typeof value === "string" &&
          value !== "y" &&
          value !== "m" &&
          value !== "n"
            ? `"${value}"`
            : value;
        lines.push(`export ${key}=${shellValue}`);
      }
    }

    return lines.join("\n");
  }

  /**
   * Serialize differences between two configs
   */
  static toDiff(oldConfig: KernelConfig, newConfig: KernelConfig): string {
    const lines: string[] = [];
    lines.push("# Configuration differences");
    lines.push("");

    const allKeys = new Set([
      ...Object.keys(oldConfig.flatConfig),
      ...Object.keys(newConfig.flatConfig),
    ]);

    for (const key of allKeys) {
      const oldValue = oldConfig.flatConfig[key];
      const newValue = newConfig.flatConfig[key];

      if (oldValue !== newValue) {
        if (oldValue === undefined) {
          lines.push(`+ ${key}=${newValue}`);
        } else if (newValue === undefined) {
          lines.push(`- ${key}=${oldValue}`);
        } else {
          lines.push(`- ${key}=${oldValue}`);
          lines.push(`+ ${key}=${newValue}`);
        }
      }
    }

    return lines.join("\n");
  }
}

export const validateKernelConfig = (data: unknown) => {
  return KernelConfigSchema.safeParse(data);
};

export const parseKernelConfigFile = (content: string): KernelConfig => {
  const parsed = KernelConfigParser.parse(content);
  const validated = KernelConfigSchema.parse(parsed);
  return validated;
};
