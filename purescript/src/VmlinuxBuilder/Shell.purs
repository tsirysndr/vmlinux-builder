-- | The host side of the build: processes, files, the current directory and
-- | one HTTP fetch, as `Aff` actions. Everything that touches the machine
-- | goes through here, so the pipeline in `VmlinuxBuilder.Build` reads as
-- | the steps themselves.
module VmlinuxBuilder.Shell
  ( run
  , runQuiet
  , capture
  , runWithYes
  , which
  , sudoPrefix
  , nproc
  , machineArch
  , fetchText
  , exists
  , readText
  , writeText
  , copyFile
  , rename
  , remove
  , chdir
  , cwd
  , argv
  , exit
  ) where

import Prelude

import Control.Promise (Promise, toAffE)
import Data.Array as Array
import Data.Either (either)
import Data.String (joinWith, trim)
import Effect (Effect)
import Effect.Aff (Aff, attempt, throwError)
import Effect.Class (liftEffect)
import Effect.Exception (error)
import Node.Encoding (Encoding(..))
import Node.FS.Aff as FS
import Node.Process as Process

foreign import runImpl :: Array String -> Effect (Promise Int)
foreign import runQuietImpl :: Array String -> Effect (Promise Int)
foreign import captureImpl :: Array String -> Effect (Promise String)
foreign import runWithYesImpl :: Array String -> Effect (Promise Int)
foreign import fetchTextImpl :: String -> Effect (Promise String)
foreign import removeImpl :: String -> Effect (Promise Unit)

-- | Run a command with the terminal as its stdio; a non-zero exit is an error.
run :: Array String -> Aff Unit
run cmd = do
  code <- toAffE (runImpl cmd)
  when (code /= 0) $ throwError $ error (joinWith " " cmd <> " exited with " <> show code)

-- | Run a command with its output discarded; `true` when it exited 0.
runQuiet :: Array String -> Aff Boolean
runQuiet cmd = either (const false) (_ == 0) <$> attempt (toAffE (runQuietImpl cmd))

-- | A command's stdout, trimmed. Its exit status is not checked, as with
-- | `make -s kernelrelease` the output is what matters.
capture :: Array String -> Aff String
capture cmd = trim <$> toAffE (captureImpl cmd)

-- | Run a command with `yes ''` on its stdin, for a `make` that may ask about
-- | new config symbols; returns the exit code.
runWithYes :: Array String -> Aff Int
runWithYes cmd = toAffE (runWithYesImpl cmd)

which :: String -> Aff Boolean
which program = runQuiet [ "which", program ]

-- | `["sudo"]` when sudo is installed, so package installs work both as root
-- | in a container and as a user on a runner.
sudoPrefix :: Aff (Array String)
sudoPrefix = which "sudo" <#> \has -> if has then [ "sudo" ] else []

nproc :: Aff String
nproc = capture [ "nproc" ]

-- | `uname -m`: `x86_64` or `aarch64`.
machineArch :: Aff String
machineArch = capture [ "uname", "-m" ]

-- | The body of an http(s) URL; a non-2xx response is an error.
fetchText :: String -> Aff String
fetchText url = toAffE (fetchTextImpl url)

exists :: String -> Aff Boolean
exists path = either (const false) (const true) <$> attempt (FS.stat path)

readText :: String -> Aff String
readText = FS.readTextFile UTF8

writeText :: String -> String -> Aff Unit
writeText = FS.writeTextFile UTF8

copyFile :: String -> String -> Aff Unit
copyFile = FS.copyFile

rename :: String -> String -> Aff Unit
rename = FS.rename

-- | `rm -rf`.
remove :: String -> Aff Unit
remove path = toAffE (removeImpl path)

chdir :: String -> Aff Unit
chdir = liftEffect <<< Process.chdir

cwd :: Aff String
cwd = liftEffect Process.cwd

-- | The arguments after `node` and the script.
argv :: Aff (Array String)
argv = liftEffect Process.argv <#> Array.drop 2

exit :: Int -> Aff Unit
exit code = liftEffect (Process.exit' code)
