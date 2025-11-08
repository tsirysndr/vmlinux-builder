
import cfg from "./.default-config" with { type: "text" };
import { parseKernelConfigFile } from './config.ts';

const config = parseKernelConfigFile(cfg);

console.log("Parsed Kernel Configuration:");
console.log(config);
