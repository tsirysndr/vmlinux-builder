# vmlinux-builder (PureScript)

A port of `build.ts` and `config.ts` to PureScript: the same kernel build tool,
and the same kernel `.config` library, with the shapes the TypeScript version
checked at run time turned into types.

It is what the `build` and `release` workflows run. The default kernel
configuration is not copied: `VmlinuxBuilder.DefaultConfig` imports the
repository's `default-config.ts` straight through Node (which strips the
TypeScript syntax itself since 23.6), so the two tools cannot disagree on
what a default kernel contains.

## Build and run

```sh
cd purescript
spago build
node bin/vmlinux-builder.js 6.17.7               # same arguments as ./build.ts
node bin/vmlinux-builder.js 6.17.7 --no-zfs --initrd
node bin/vmlinux-builder.js --repo https://github.com/tsirysndr/linux-orangepi \
  --branch orange-pi-6.6-sun60iw2 --defconfig sun60iw2_defconfig
spago test
```

Needs Node ≥ 23.6, PureScript 0.15 and Spago 1.x (`npm install` fetches the
last two). The kernel's own toolchain is the one the root README lists; OpenZFS
also wants `autoconf automake libtool gawk`.

## Layout

| Module                          | What it is                                                                                                   |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| `Main`                          | Parse the command line, run the build, exit non-zero with the failure shown.                                |
| `VmlinuxBuilder.Options`        | Every flag of `build.ts`, parsed into `Options`: a `Source` (upstream version or custom tree), a `ConfigMode`, the artifact switches. |
| `VmlinuxBuilder.Version`        | `KernelVersion`: a release tag (`v6.16.2`) or a maintenance branch (`linux-6.16.y`), and the artifact label. |
| `VmlinuxBuilder.Build`          | The pipeline as named `Aff` steps: toolchain, checkout, `.config`, prepare, OpenZFS, `make vmlinux`, Image, uImage, initrd, modules. |
| `VmlinuxBuilder.Zfs`            | The OpenZFS decisions and `.config` edits, pure and tested.                                                  |
| `VmlinuxBuilder.Shell`          | Processes, files, cwd and one HTTP fetch, as `Aff` — the only module that touches the host.                  |
| `VmlinuxBuilder.Config`         | The `config.ts` port: parse, query, serialize to `.config`/JSON/TOML/YAML/Makefile/shell/diff.                |
| `VmlinuxBuilder.Toml`           | The sliver of TOML a kernel config needs (the registry has no TOML package).                                 |
| `VmlinuxBuilder.DefaultConfig`  | The bundled config, imported from `../default-config.ts`.                                                    |

## The config library

```purescript
import VmlinuxBuilder.Config as Config

config = Config.parse text                       -- total; unknown lines are skipped
Config.getValue "CONFIG_NR_CPUS" config          -- Just (Numeric 64)
Config.isEnabled "CONFIG_SMP" config             -- y or m
Config.serialize Config.defaultSerializeOptions { sortKeys = true } config
Config.toJson config / Config.fromJson text      -- Either String KernelConfig
Config.toToml config / Config.fromToml text
Config.toYaml, toMakefile, toEnabledOnly, toShellScript, toDiff
```

`ConfigValue` is a sum type (`Tristate Y|M|N`, `Numeric`, `Text`, `Flag`); a
not-set option is `Nothing`. `FlatConfig` keeps the file's order beside the
map, so a written-back config is in the kernel's order unless `sortKeys`.

Three things differ from `config.ts`, on purpose:

- The file's own header is not a section. `config.ts` turned
  `# Automatically generated file; DO NOT EDIT.` into a section that swallowed
  every other one; here it is skipped and the `Linux/<arch> <version>` line
  fills `version`, which the TypeScript model had but never set.
- A top-level `# end of` closes its section, so sections nest as the kernel
  wrote them, and options written before the first section survive a
  serialize/parse round trip.
- `0` and `""` are values. `config.ts` wrote both back as "is not set".

There is no `validate`/`strict`: a `KernelConfig` cannot be malformed, and
`fromJson`/`fromToml` return `Either` instead of an empty config.
