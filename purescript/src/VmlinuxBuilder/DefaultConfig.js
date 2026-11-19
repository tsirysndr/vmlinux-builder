// One source of truth: the same default config build.ts bundles. The path is
// relative to both src/VmlinuxBuilder/ and output/VmlinuxBuilder.DefaultConfig/,
// which sit at the same depth under purescript/.
import cfg from "../../../default-config.ts";

export const defaultConfig = cfg;
