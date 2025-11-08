import { KernelConfigSerializer, parseKernelConfigFile } from "./config.ts";
import cfg from "./default-config.ts";

const config = parseKernelConfigFile(cfg);

console.log("# Parsed Kernel Configuration:");
console.log(KernelConfigSerializer.toTOML(config));
