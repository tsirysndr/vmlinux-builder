-- | `vmlinux-builder`: parse the command line, run the build, exit with
-- | the failure shown.
module Main (main) where

import Prelude

import Data.Either (Either(..))
import Effect (Effect)
import Effect.Aff (attempt, launchAff_)
import Effect.Exception (message)
import VmlinuxBuilder.Build (build)
import VmlinuxBuilder.Log as Log
import VmlinuxBuilder.Options (parseArgs, usage)
import VmlinuxBuilder.Shell (argv, exit)

main :: Effect Unit
main = launchAff_ do
  args <- argv
  case parseArgs args of
    Left err -> do
      Log.warn ("Error: " <> err)
      Log.info usage
      exit 1
    Right opts -> do
      outcome <- attempt (build opts)
      case outcome of
        Left err -> do
          Log.warn ("Error: " <> message err)
          exit 1
        Right _ -> pure unit
