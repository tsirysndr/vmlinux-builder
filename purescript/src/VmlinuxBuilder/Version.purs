-- | Which upstream kernel to build, and what to call the result.
module VmlinuxBuilder.Version
  ( KernelVersion(..)
  , parseKernelVersion
  , gitRef
  , versionLabel
  ) where

import Prelude

import Data.Either (Either, note)
import Data.Maybe (Maybe(..), fromMaybe)
import Data.String (Pattern(..), stripPrefix, stripSuffix)
import Data.String.Regex (Regex, match)
import Data.String.Regex.Flags (noFlags)
import Data.String.Regex.Unsafe (unsafeRegex)

-- | `Release "6.16.2"` is the tag `v6.16.2`; `Maintenance "6.16"` is the
-- | branch `linux-6.16.y`, whatever its latest patch level is today.
data KernelVersion
  = Release String
  | Maintenance String

derive instance Eq KernelVersion

instance Show KernelVersion where
  show (Release v) = "(Release " <> show v <> ")"
  show (Maintenance v) = "(Maintenance " <> show v <> ")"

versionRx :: Regex
versionRx = unsafeRegex "^[0-9]+\\.[0-9]+(\\.(y|[0-9]+))?$" noFlags

-- | `6.1`, `6.1.12`, `6.1.y`, with or without a leading `v`.
parseKernelVersion :: String -> Either String KernelVersion
parseKernelVersion input = do
  let num = fromMaybe input (stripPrefix (Pattern "v") input)
  _ <- note ("invalid kernel version " <> show input <> ": expected X.Y, X.Y.Z or X.Y.y")
    (match versionRx num)
  pure case stripSuffix (Pattern ".y") num of
    Just base -> Maintenance base
    Nothing -> Release num

-- | The git ref to check out.
gitRef :: KernelVersion -> String
gitRef (Release v) = "v" <> v
gitRef (Maintenance v) = "linux-" <> v <> ".y"

-- | The label in `vmlinux-<label>.<arch>`.
versionLabel :: KernelVersion -> String
versionLabel (Release v) = v
versionLabel (Maintenance v) = v
