-- | The sliver of TOML a kernel config needs: top-level keys and flat
-- | `[tables]` of strings, integers, floats and booleans. The registry has no
-- | TOML package, and `config.ts` only ever wrote and read this shape, so a
-- | small parser the types can vouch for beats a foreign one.
module VmlinuxBuilder.Toml
  ( TomlValue(..)
  , TomlTable
  , TomlDoc
  , render
  , parse
  ) where

import Prelude

import Data.Array (snoc)
import Data.Array as Array
import Data.Either (Either(..), note)
import Data.Foldable (foldM)
import Data.Int as Int
import Data.Maybe (Maybe(..), fromMaybe, isJust)
import Data.Number as Number
import Data.String (Pattern(..), joinWith, split, stripPrefix, stripSuffix, trim)
import Data.String as String
import Data.String.Regex (Regex, match, replace)
import Data.String.Regex.Flags (global, noFlags)
import Data.String.Regex.Unsafe (unsafeRegex)
import Data.Tuple (Tuple(..))

data TomlValue
  = TString String
  | TInt Int
  | TFloat Number
  | TBool Boolean

derive instance Eq TomlValue

instance Show TomlValue where
  show (TString s) = "(TString " <> show s <> ")"
  show (TInt n) = "(TInt " <> show n <> ")"
  show (TFloat f) = "(TFloat " <> show f <> ")"
  show (TBool b) = "(TBool " <> show b <> ")"

type TomlTable = Array (Tuple String TomlValue)

-- | Keys before any `[table]`, then each table in order.
type TomlDoc =
  { root :: TomlTable
  , tables :: Array (Tuple String TomlTable)
  }

bareKeyRx :: Regex
bareKeyRx = unsafeRegex "^[A-Za-z0-9_-]+$" noFlags

intRx :: Regex
intRx = unsafeRegex "^[-+]?\\d+$" noFlags

floatRx :: Regex
floatRx = unsafeRegex "^[-+]?\\d+\\.\\d+$" noFlags

tableRx :: Regex
tableRx = unsafeRegex "^\\[\\s*([A-Za-z0-9_.-]+)\\s*\\]$" noFlags

quoteRx :: Regex
quoteRx = unsafeRegex "[\"\\\\]" global

render :: TomlDoc -> String
render doc = joinWith "\n" (rootLines <> tableLines) <> "\n"
  where
  rootLines = doc.root <#> pair
  tableLines = doc.tables >>= \(Tuple name entries) ->
    [ "", "[" <> name <> "]" ] <> (entries <#> pair)
  pair (Tuple k v) = key k <> " = " <> value v
  key k = if isJust (match bareKeyRx k) then k else quote k
  value = case _ of
    TString s -> quote s
    TInt n -> show n
    TFloat f -> show f
    TBool b -> if b then "true" else "false"
  quote s = "\"" <> replace quoteRx "\\$&" s <> "\""

-- | Parse the subset. Anything else — a nested table header, an array, a
-- | key with spaces, a dangling `=` — is an error naming the line.
parse :: String -> Either String TomlDoc
parse text = finish <$> foldM step { root: [], tables: [], current: Nothing } (split (Pattern "\n") text)
  where
  finish st = { root: st.root, tables: fromMaybe st.tables (push st) }

  push st = st.current <#> \t -> snoc st.tables t

  step st raw =
    let
      line = trim (stripComment raw)
    in
      if line == "" then Right st
      else case match tableRx line of
        Just groups -> case join (Array.index (Array.fromFoldable groups) 1) of
          Just name -> Right (st { tables = fromMaybe st.tables (push st), current = Just (Tuple name []) })
          Nothing -> Left ("invalid table header: " <> line)
        Nothing -> case String.indexOf (Pattern "=") line of
          Nothing -> Left ("expected key = value, got: " <> line)
          Just i -> do
            k <- parseKey (trim (String.take i line))
            v <- parseValue (trim (String.drop (i + 1) line))
            pure case st.current of
              Just (Tuple name entries) -> st { current = Just (Tuple name (snoc entries (Tuple k v))) }
              Nothing -> st { root = snoc st.root (Tuple k v) }

  -- A `#` outside quotes starts a comment.
  stripComment s = case String.indexOf (Pattern "#") s of
    Just i | not (inQuotes (String.take i s)) -> String.take i s
    _ -> s
  inQuotes prefix = Int.odd (Array.length (String.split (Pattern "\"") prefix) - 1)

  parseKey k
    | isJust (match bareKeyRx k) = Right k
    | Just inner <- stripPrefix (Pattern "\"") k >>= stripSuffix (Pattern "\"") = Right inner
    | otherwise = Left ("invalid key: " <> show k)

  parseValue v
    | v == "true" = Right (TBool true)
    | v == "false" = Right (TBool false)
    | isJust (match intRx v) = TInt <$> note ("integer out of range: " <> v) (Int.fromString v)
    | isJust (match floatRx v) = TFloat <$> note ("invalid float: " <> v) (Number.fromString v)
    | Just inner <- stripPrefix (Pattern "\"") v >>= stripSuffix (Pattern "\"") = Right (TString (unescape inner))
    | Just inner <- stripPrefix (Pattern "'") v >>= stripSuffix (Pattern "'") = Right (TString inner)
    | otherwise = Left ("invalid value: " <> v)

  unescape = String.replaceAll (Pattern "\\\"") (String.Replacement "\"")
    >>> String.replaceAll (Pattern "\\\\") (String.Replacement "\\")
