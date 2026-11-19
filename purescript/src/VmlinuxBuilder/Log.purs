-- | Coloured progress lines, the same four tones `build.ts` used.
module VmlinuxBuilder.Log
  ( info
  , step
  , success
  , warn
  , cyan
  ) where

import Prelude

import Effect.Aff (Aff)
import Effect.Class (liftEffect)
import Effect.Console as Console

paint :: String -> String -> String
paint code s = "\x1b[" <> code <> "m" <> s <> "\x1b[0m"

-- | Highlight a value inside a message.
cyan :: String -> String
cyan = paint "36"

info :: String -> Aff Unit
info = liftEffect <<< Console.log

-- | A phase starting (magenta, as before).
step :: String -> Aff Unit
step = liftEffect <<< Console.log <<< paint "35"

success :: String -> Aff Unit
success = liftEffect <<< Console.log <<< paint "32"

warn :: String -> Aff Unit
warn = liftEffect <<< Console.log <<< paint "33"
