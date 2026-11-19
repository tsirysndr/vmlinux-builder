-- | The bundled kernel configuration, shared with `build.ts`.
-- |
-- | There is one copy of the 3,600-line config in this repository:
-- | `default-config.ts` at the root. The foreign module imports it directly
-- | (Node strips the TypeScript syntax itself since 23.6), so the two tools
-- | cannot drift apart on what a default kernel contains.
module VmlinuxBuilder.DefaultConfig (defaultConfig) where

foreign import defaultConfig :: String
