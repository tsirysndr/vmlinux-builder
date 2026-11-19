-- | Linux kernel `.config` files as data: parsed into a typed model, queried,
-- | and written back out in the kernel's own format or in JSON, TOML, YAML,
-- | Makefile, shell and diff forms.
-- |
-- | This is the PureScript port of `config.ts`. Where that file validated its
-- | shapes at run time with Zod, here the types are the validation: a
-- | `KernelConfig` cannot be malformed, so the `validate`/`strict` switches
-- | have no equivalent and the parser is total. Two of the original's
-- | accidents are not reproduced: a top-level `# end of` line closes its
-- | section here (so a file's sections nest as the kernel wrote them), and a
-- | value of `0` or `""` is a value, not "not set".
module VmlinuxBuilder.Config
  ( Tristate(..)
  , ConfigValue(..)
  , ConfigEntry
  , ConfigSection(..)
  , BuildInfo
  , KernelConfig
  , emptyConfig
  , FlatConfig
  , emptyFlat
  , flatFromEntries
  , flatEntries
  , flatKeys
  , flatLookup
  , flatInsert
  , flatSize
  , parse
  , getValue
  , isEnabled
  , ProcessorConfig
  , SecurityConfig
  , NetworkingConfig
  , FilesystemConfig
  , extractProcessorConfig
  , extractSecurityConfig
  , extractNetworkingConfig
  , extractFilesystemConfig
  , FormatStyle(..)
  , SerializeOptions
  , defaultSerializeOptions
  , serialize
  , fromObject
  , toObject
  , toJson
  , fromJson
  , toToml
  , fromToml
  , toYaml
  , toMakefile
  , toEnabledOnly
  , toShellScript
  , toDiff
  , renderValue
  ) where

import Prelude

import Data.Argonaut.Core (Json, fromBoolean, fromString, jsonNull, stringify, toArray, toBoolean, toNull, toString)
import Data.Argonaut.Core as Json
import Data.Argonaut.Parser (jsonParser)
import Data.Array (catMaybes, mapMaybe, nub, snoc)
import Data.Array as Array
import Data.Either (Either(..), note)
import Data.Foldable (foldl)
import Data.Int (fromStringAs, hexadecimal)
import Data.Int as Int
import Data.List (List(..), (:))
import Data.Map (Map)
import Data.Map as Map
import Data.Set (Set)
import Data.Set as Set
import Data.Maybe (Maybe(..), fromMaybe, isJust, maybe)
import Data.String (Pattern(..), contains, joinWith, split, stripPrefix, stripSuffix, trim)
import Data.String as String
import Data.String.Regex (Regex, match)
import Data.String.Regex.Unsafe (unsafeRegex)
import Data.String.Regex.Flags (noFlags)
import Data.Traversable (traverse)
import Data.Tuple (Tuple(..), fst, snd)
import Foreign.Object as Object
import VmlinuxBuilder.Toml (TomlValue(..))
import VmlinuxBuilder.Toml as Toml

-- ---------------------------------------------------------------------------
-- The model
-- ---------------------------------------------------------------------------

-- | `y` built in, `m` a module, `n` explicitly off.
data Tristate = Y | M | N

derive instance Eq Tristate
derive instance Ord Tristate

instance Show Tristate where
  show Y = "y"
  show M = "m"
  show N = "n"

-- | What a `CONFIG_*=` line carries. `Flag` never comes from a `.config`
-- | file; it is how a JSON or TOML document says true/false, and it is what
-- | `toObject true` produces.
data ConfigValue
  = Tristate Tristate
  | Numeric Int
  | Text String
  | Flag Boolean

derive instance Eq ConfigValue

instance Show ConfigValue where
  show (Tristate t) = "(Tristate " <> show t <> ")"
  show (Numeric n) = "(Numeric " <> show n <> ")"
  show (Text s) = "(Text " <> show s <> ")"
  show (Flag b) = "(Flag " <> show b <> ")"

-- | One option. `Nothing` is a `# CONFIG_X is not set` line.
type ConfigEntry =
  { key :: String
  , value :: Maybe ConfigValue
  , comment :: Maybe String
  }

-- | A `# Name … # end of Name` block, nested as the kernel wrote it.
newtype ConfigSection = ConfigSection
  { name :: String
  , entries :: Array ConfigEntry
  , subsections :: Array ConfigSection
  }

derive instance Eq ConfigSection

instance Show ConfigSection where
  show (ConfigSection s) =
    "(ConfigSection " <> show s.name <> " " <> show (Array.length s.entries)
      <> " entries, " <> show (Array.length s.subsections) <> " subsections)"

type BuildInfo =
  { compiler :: Maybe String
  , gccVersion :: Maybe String
  , buildSalt :: Maybe String
  }

-- | Every option by key, in file order, beside the section tree.
type KernelConfig =
  { version :: Maybe String
  , buildInfo :: Maybe BuildInfo
  , sections :: Array ConfigSection
  , flatConfig :: FlatConfig
  }

emptyConfig :: KernelConfig
emptyConfig = { version: Nothing, buildInfo: Nothing, sections: [], flatConfig: emptyFlat }

-- | The options keyed by name, remembering the order they appeared in: a
-- | `Map` alone would sort them, and a written-back `.config` should keep
-- | the kernel's order unless asked to sort.
newtype FlatConfig = FlatConfig
  { order :: Array String
  , values :: Map String (Maybe ConfigValue)
  }

derive instance Eq FlatConfig

instance Show FlatConfig where
  show f = "(FlatConfig " <> show (flatEntries f) <> ")"

emptyFlat :: FlatConfig
emptyFlat = FlatConfig { order: [], values: Map.empty }

-- | Insert or replace; a new key goes last.
flatInsert :: String -> Maybe ConfigValue -> FlatConfig -> FlatConfig
flatInsert key value (FlatConfig f) = FlatConfig
  { order: if Map.member key f.values then f.order else snoc f.order key
  , values: Map.insert key value f.values
  }

flatFromEntries :: Array (Tuple String (Maybe ConfigValue)) -> FlatConfig
flatFromEntries = foldl (\acc (Tuple k v) -> flatInsert k v acc) emptyFlat

-- | Every option in order. `Nothing` is "not set".
flatEntries :: FlatConfig -> Array (Tuple String (Maybe ConfigValue))
flatEntries (FlatConfig f) = f.order <#> \k -> Tuple k (join (Map.lookup k f.values))

flatKeys :: FlatConfig -> Array String
flatKeys (FlatConfig f) = f.order

-- | `Nothing` when the key is absent *or* recorded as not set; the two are
-- | one state to every consumer here, as they were in the original.
flatLookup :: String -> FlatConfig -> Maybe ConfigValue
flatLookup key (FlatConfig f) = join (Map.lookup key f.values)

flatSize :: FlatConfig -> Int
flatSize (FlatConfig f) = Array.length f.order

-- ---------------------------------------------------------------------------
-- Parsing
-- ---------------------------------------------------------------------------

sectionRx :: Regex
sectionRx = unsafeRegex "^#\\s*(.+)$" noFlags

notSetRx :: Regex
notSetRx = unsafeRegex "^#\\s*(CONFIG_\\w+)\\s+is not set" noFlags

assignRx :: Regex
assignRx = unsafeRegex "^(CONFIG_\\w+)=(.+)$" noFlags

-- | The second line of the kernel's own header, which names the version.
headerVersionRx :: Regex
headerVersionRx = unsafeRegex "^Linux/\\S+ (\\S+) Kernel Configuration$" noFlags

intRx :: Regex
intRx = unsafeRegex "^-?\\d+$" noFlags

hexRx :: Regex
hexRx = unsafeRegex "^0x[0-9a-fA-F]+$" noFlags

-- | The first capture group of a match, if it matched.
group :: Int -> Regex -> String -> Maybe String
group n rx s = match rx s >>= \groups -> join (Array.index (Array.fromFoldable groups) n)

-- | Parser state: the options so far, the finished top-level sections, and
-- | the sections still open — head is the one entries currently go into.
type ParseState =
  { flat :: FlatConfig
  , sections :: Array ConfigSection
  , open :: List ConfigSection
  , version :: Maybe String
  }

-- | Parse `.config` text. Total: unknown lines are skipped.
parse :: String -> KernelConfig
parse content =
  let
    final = closeAll (foldl step { flat: emptyFlat, sections: [], open: Nil, version: Nothing } (split (Pattern "\n") content))
    flat = final.flat
    gcc = case flatLookup "CONFIG_GCC_VERSION" flat of
      Just v -> Just (renderValue v)
      Nothing -> Nothing
  in
    { version: final.version
    , buildInfo: Just
        { compiler: textOf (flatLookup "CONFIG_CC_VERSION_TEXT" flat)
        , gccVersion: gcc
        , buildSalt: textOf (flatLookup "CONFIG_BUILD_SALT" flat)
        }
    , sections: final.sections
    , flatConfig: flat
    }
  where
  textOf (Just (Text s)) = Just s
  textOf _ = Nothing

  step :: ParseState -> String -> ParseState
  step st raw =
    let
      line = trim raw
    in
      if line == "" then st
      else if String.take 1 line == "#" && not (contains (Pattern "CONFIG_") line) then
        case group 1 sectionRx line of
          Nothing -> st
          -- The file's own header is not a section: the kernel writes
          -- `# Automatically generated file; DO NOT EDIT.` and
          -- `# Linux/<arch> <version> Kernel Configuration` in the same
          -- comment shape, and the version is worth keeping.
          Just "Automatically generated file; DO NOT EDIT." -> st
          Just name | Just version <- group 1 headerVersionRx name -> st { version = Just version }
          Just name -> case stripPrefix (Pattern "end of") name of
            Just _ -> closeOne st
            Nothing -> st { open = ConfigSection { name, entries: [], subsections: [] } : st.open }
      else case group 1 notSetRx line of
        Just key -> record st key Nothing
        Nothing -> case Tuple <$> group 1 assignRx line <*> group 2 assignRx line of
          Just (Tuple key raw') -> record st key (Just (parseValue raw'))
          Nothing -> st

  record st key value =
    let
      entry = { key, value, comment: Nothing }
      open' = case st.open of
        (ConfigSection s) : rest -> ConfigSection (s { entries = snoc s.entries entry }) : rest
        Nil -> Nil
    in
      st { flat = flatInsert key value st.flat, open = open' }

  -- Close the innermost open section into its parent, or into the top level.
  closeOne st = case st.open of
    done : (ConfigSection parent) : rest ->
      st { open = ConfigSection (parent { subsections = snoc parent.subsections done }) : rest }
    done : Nil -> st { open = Nil, sections = snoc st.sections done }
    Nil -> st

  closeAll st = case st.open of
    Nil -> st
    _ -> closeAll (closeOne st)

parseValue :: String -> ConfigValue
parseValue raw = case raw of
  "y" -> Tristate Y
  "m" -> Tristate M
  "n" -> Tristate N
  _
    | isJust (match intRx raw) -> maybe (Text raw) Numeric (Int.fromString raw)
    | isJust (match hexRx raw) ->
        maybe (Text raw) Numeric (fromStringAs hexadecimal (String.drop 2 raw))
    | otherwise -> Text (unquote raw)
  where
  unquote s = fromMaybe s (stripPrefix (Pattern "\"") s >>= stripSuffix (Pattern "\""))

-- ---------------------------------------------------------------------------
-- Queries
-- ---------------------------------------------------------------------------

getValue :: String -> KernelConfig -> Maybe ConfigValue
getValue key config = flatLookup key config.flatConfig

-- | Built in or a module.
isEnabled :: String -> KernelConfig -> Boolean
isEnabled key config = case getValue key config of
  Just (Tristate Y) -> true
  Just (Tristate M) -> true
  _ -> false

on :: String -> KernelConfig -> Boolean
on key config = getValue key config == Just (Tristate Y)

number :: String -> KernelConfig -> Maybe Int
number key config = case getValue key config of
  Just (Numeric n) -> Just n
  _ -> Nothing

type ProcessorConfig =
  { smp :: Boolean
  , nrCpus :: Maybe Int
  , x86_64 :: Boolean
  , numa :: Boolean
  , preempt :: Boolean
  , preemptVoluntary :: Boolean
  , preemptNone :: Boolean
  }

type SecurityConfig =
  { security :: Boolean
  , selinux :: Boolean
  , apparmor :: Boolean
  , smack :: Boolean
  , seccomp :: Boolean
  , stackProtector :: Boolean
  , fortifySource :: Boolean
  }

type NetworkingConfig =
  { net :: Boolean
  , inet :: Boolean
  , ipv6 :: Boolean
  , netfilter :: Boolean
  , packet :: Boolean
  , unix :: Boolean
  }

type FilesystemConfig =
  { ext4 :: Boolean
  , xfs :: Boolean
  , btrfs :: Boolean
  , nfs :: Boolean
  , tmpfs :: Boolean
  }

extractProcessorConfig :: KernelConfig -> ProcessorConfig
extractProcessorConfig c =
  { smp: on "CONFIG_SMP" c
  , nrCpus: number "CONFIG_NR_CPUS" c
  , x86_64: on "CONFIG_X86_64" c
  , numa: on "CONFIG_NUMA" c
  , preempt: on "CONFIG_PREEMPT" c
  , preemptVoluntary: on "CONFIG_PREEMPT_VOLUNTARY" c
  , preemptNone: on "CONFIG_PREEMPT_NONE" c
  }

extractSecurityConfig :: KernelConfig -> SecurityConfig
extractSecurityConfig c =
  { security: on "CONFIG_SECURITY" c
  , selinux: on "CONFIG_SECURITY_SELINUX" c
  , apparmor: on "CONFIG_SECURITY_APPARMOR" c
  , smack: on "CONFIG_SECURITY_SMACK" c
  , seccomp: on "CONFIG_SECCOMP" c
  , stackProtector: on "CONFIG_STACKPROTECTOR" c
  , fortifySource: on "CONFIG_FORTIFY_SOURCE" c
  }

extractNetworkingConfig :: KernelConfig -> NetworkingConfig
extractNetworkingConfig c =
  { net: on "CONFIG_NET" c
  , inet: on "CONFIG_INET" c
  , ipv6: on "CONFIG_IPV6" c
  , netfilter: on "CONFIG_NETFILTER" c
  , packet: on "CONFIG_PACKET" c
  , unix: on "CONFIG_UNIX" c
  }

extractFilesystemConfig :: KernelConfig -> FilesystemConfig
extractFilesystemConfig c =
  { ext4: on "CONFIG_EXT4_FS" c
  , xfs: on "CONFIG_XFS_FS" c
  , btrfs: on "CONFIG_BTRFS_FS" c
  , nfs: on "CONFIG_NFS_FS" c
  , tmpfs: on "CONFIG_TMPFS" c
  }

-- ---------------------------------------------------------------------------
-- Writing `.config`
-- ---------------------------------------------------------------------------

-- | `Kernel` quotes a string only when it has to; `Quoted` always does.
data FormatStyle = Kernel | Quoted

derive instance Eq FormatStyle

type SerializeOptions =
  { preserveSections :: Boolean
  , includeComments :: Boolean
  , sortKeys :: Boolean
  , addHeader :: Boolean
  , formatStyle :: FormatStyle
  }

defaultSerializeOptions :: SerializeOptions
defaultSerializeOptions =
  { preserveSections: true
  , includeComments: true
  , sortKeys: false
  , addHeader: true
  , formatStyle: Kernel
  }

-- | A value as the kernel writes it after `=`: `y`, a number, a raw string.
renderValue :: ConfigValue -> String
renderValue = case _ of
  Tristate t -> show t
  Numeric n -> show n
  Text s -> s
  Flag b -> if b then "y" else "n"

serialize :: SerializeOptions -> KernelConfig -> String
serialize opts config = joinWith "\n" (header <> body)
  where
  header =
    if opts.addHeader then
      [ "#", "# Automatically generated file; DO NOT EDIT." ]
        <> catMaybes
          [ config.buildInfo >>= _.compiler <#> \c -> "# " <> c
          , config.buildInfo >>= _.buildSalt <#> \_ -> "# Linux Kernel Configuration"
          ]
        <> [ "#", "" ]
    else []

  -- Options outside every section (the kernel writes a few before the first
  -- one) come first, so a file with sections still carries all of them.
  body =
    if opts.preserveSections && not (Array.null config.sections) then
      loose <> (config.sections >>= section)
    else
      flatLines

  covered :: Set String
  covered = Set.fromFoldable (config.sections >>= sectionKeys)

  sectionKeys (ConfigSection s) = (s.entries <#> _.key) <> (s.subsections >>= sectionKeys)

  loose = flatEntries config.flatConfig
    # Array.filter (\(Tuple key _) -> not (Set.member key covered))
    >>= \(Tuple key value) -> entryLines { key, value, comment: Nothing }

  flatLines =
    let
      entries = flatEntries config.flatConfig
      ordered = if opts.sortKeys then Array.sortWith fst entries else entries
    in
      ordered >>= \(Tuple key value) -> entryLines { key, value, comment: Nothing }

  section (ConfigSection s) =
    (if opts.includeComments then [ "", "#", "# " <> s.name, "#" ] else [])
      <> (s.entries >>= entryLines)
      <> (s.subsections >>= section)
      <> (if opts.includeComments then [ "# end of " <> s.name ] else [])

  entryLines e =
    let
      base = case e.value of
        Nothing -> "# " <> e.key <> " is not set"
        Just (Flag false) -> "# " <> e.key <> " is not set"
        Just (Text s) ->
          let
            needsQuotes = opts.formatStyle == Quoted
              || contains (Pattern " ") s
              || contains (Pattern "#") s
              || contains (Pattern "=") s
          in
            e.key <> "=" <> (if needsQuotes then "\"" <> s <> "\"" else s)
        Just v -> e.key <> "=" <> renderValue v
    in
      case e.comment of
        Just c | opts.includeComments -> [ base <> "  # " <> c ]
        _ -> [ base ]

-- ---------------------------------------------------------------------------
-- Objects
-- ---------------------------------------------------------------------------

-- | A config from plain key/value pairs: `Flag true` is `y`, `Flag false`
-- | and `Nothing` are not set, anything that is not a `CONFIG_` key is
-- | ignored. The sections are empty: an object has no layout.
fromObject :: Array (Tuple String (Maybe ConfigValue)) -> KernelConfig
fromObject pairs = emptyConfig { flatConfig = flatFromEntries (mapMaybe keep pairs) }
  where
  keep (Tuple key value)
    | isJust (stripPrefix (Pattern "CONFIG_") key) = Just (Tuple key (normalize value))
    | otherwise = Nothing
  normalize (Just (Flag true)) = Just (Tristate Y)
  normalize (Just (Flag false)) = Nothing
  normalize v = v

-- | The options as pairs; with `booleanStyle`, `y`/`m` become `Flag true`
-- | and `n`/not set become `Flag false`, so every key has a value.
toObject :: Boolean -> KernelConfig -> Array (Tuple String (Maybe ConfigValue))
toObject booleanStyle config = flatEntries config.flatConfig <#> \(Tuple k v) ->
  Tuple k (if booleanStyle then Just (asFlag v) else v)
  where
  asFlag (Just (Tristate Y)) = Flag true
  asFlag (Just (Tristate M)) = Flag true
  asFlag (Just (Tristate N)) = Flag false
  asFlag Nothing = Flag false
  asFlag (Just other) = other

-- ---------------------------------------------------------------------------
-- JSON
-- ---------------------------------------------------------------------------

valueToJson :: Maybe ConfigValue -> Json
valueToJson = case _ of
  Nothing -> jsonNull
  Just (Tristate t) -> fromString (show t)
  Just (Numeric n) -> Json.fromNumber (Int.toNumber n)
  Just (Text s) -> fromString s
  Just (Flag b) -> fromBoolean b

valueFromJson :: Json -> Either String (Maybe ConfigValue)
valueFromJson j
  | isJust (toNull j) = Right Nothing
  | Just b <- toBoolean j = Right (Just (Flag b))
  | Just n <- Json.toNumber j = Right (Just (maybe (Text (show n)) Numeric (Int.fromNumber n)))
  | Just s <- toString j = Right (Just (tristateOrText s))
  | otherwise = Left "a config value must be a string, number, boolean or null"

-- | `y`/`m`/`n` as a tristate, anything else as text.
tristateOrText :: String -> ConfigValue
tristateOrText = case _ of
  "y" -> Tristate Y
  "m" -> Tristate M
  "n" -> Tristate N
  s -> Text s

configToJson :: KernelConfig -> Json
configToJson config = Json.fromObject $ Object.fromFoldable $ catMaybes
  [ config.version <#> \v -> Tuple "version" (fromString v)
  , config.buildInfo <#> \b -> Tuple "buildInfo" (buildInfoJson b)
  , Just (Tuple "sections" (Json.fromArray (sectionJson <$> config.sections)))
  , Just (Tuple "flatConfig" (flatJson config.flatConfig))
  ]
  where
  buildInfoJson b = Json.fromObject $ Object.fromFoldable $ catMaybes
    [ b.compiler <#> Tuple "compiler" <<< fromString
    , b.gccVersion <#> Tuple "gccVersion" <<< fromString
    , b.buildSalt <#> Tuple "buildSalt" <<< fromString
    ]
  flatJson flat = Json.fromObject $ Object.fromFoldable $ flatEntries flat <#> \(Tuple k v) -> Tuple k (valueToJson v)
  sectionJson (ConfigSection s) = Json.fromObject $ Object.fromFoldable
    [ Tuple "name" (fromString s.name)
    , Tuple "entries" (Json.fromArray (entryJson <$> s.entries))
    , Tuple "subsections" (Json.fromArray (sectionJson <$> s.subsections))
    ]
  entryJson e = Json.fromObject $ Object.fromFoldable $ catMaybes
    [ Just (Tuple "key" (fromString e.key))
    , Just (Tuple "value" (valueToJson e.value))
    , e.comment <#> Tuple "comment" <<< fromString
    ]

configFromJson :: Json -> Either String KernelConfig
configFromJson j = do
  obj <- note "a kernel config is a JSON object" (Json.toObject j)
  let field k = Object.lookup k obj
  version <- traverse (note "version must be a string" <<< toString) (field "version")
  buildInfo <- traverse buildInfoFrom (field "buildInfo")
  sections <- maybe (Right []) (\s -> note "sections must be an array" (toArray s) >>= traverse sectionFrom) (field "sections")
  flat <- maybe (Right emptyFlat) flatFrom (field "flatConfig")
  pure { version, buildInfo, sections, flatConfig: flat }
  where
  str k obj = traverse (note (k <> " must be a string") <<< toString) (Object.lookup k obj)
  buildInfoFrom bj = do
    obj <- note "buildInfo must be an object" (Json.toObject bj)
    compiler <- str "compiler" obj
    gccVersion <- str "gccVersion" obj
    buildSalt <- str "buildSalt" obj
    pure { compiler, gccVersion, buildSalt }
  flatFrom fj = do
    obj <- note "flatConfig must be an object" (Json.toObject fj)
    pairs <- traverse (\(Tuple k v) -> Tuple k <$> valueFromJson v) (Object.toUnfoldable obj)
    pure (flatFromEntries pairs)
  sectionFrom sj = do
    obj <- note "a section must be an object" (Json.toObject sj)
    name <- note "a section needs a name" (Object.lookup "name" obj >>= toString)
    entries <- maybe (Right []) (\e -> note "entries must be an array" (toArray e) >>= traverse entryFrom) (Object.lookup "entries" obj)
    subsections <- maybe (Right []) (\s -> note "subsections must be an array" (toArray s) >>= traverse sectionFrom) (Object.lookup "subsections" obj)
    pure (ConfigSection { name, entries, subsections })
  entryFrom ej = do
    obj <- note "an entry must be an object" (Json.toObject ej)
    key <- note "an entry needs a key" (Object.lookup "key" obj >>= toString)
    value <- maybe (Right Nothing) valueFromJson (Object.lookup "value" obj)
    comment <- str "comment" obj
    pure { key, value, comment }

-- | JSON text. Not-set options are `null`.
toJson :: KernelConfig -> String
toJson = stringify <<< configToJson

fromJson :: String -> Either String KernelConfig
fromJson text = jsonParser text >>= configFromJson

-- ---------------------------------------------------------------------------
-- TOML
-- ---------------------------------------------------------------------------

-- | A `[buildInfo]` table and a `[config]` table. Not-set options are
-- | written as `false`, which `fromToml` reads back as not set.
toToml :: KernelConfig -> String
toToml config = Toml.render
  { root: []
  , tables:
      [ Tuple "buildInfo" (maybe [] buildInfoTable config.buildInfo)
      , Tuple "config" (flatEntries config.flatConfig <#> \(Tuple k v) -> Tuple k (tomlValue v))
      ]
  }
  where
  buildInfoTable b = catMaybes
    [ b.compiler <#> Tuple "compiler" <<< TString
    , b.gccVersion <#> Tuple "gccVersion" <<< TString
    , b.buildSalt <#> Tuple "buildSalt" <<< TString
    ]
  tomlValue = case _ of
    Nothing -> TBool false
    Just (Tristate t) -> TString (show t)
    Just (Numeric n) -> TInt n
    Just (Text s) -> TString s
    Just (Flag b) -> TBool b

-- | `CONFIG_` keys at the top level and in a `[config]` table; a
-- | `[buildInfo]` table fills `buildInfo`.
fromToml :: String -> Either String KernelConfig
fromToml text = do
  doc <- Toml.parse text
  let
    table name = fromMaybe [] (snd <$> Array.find (\t -> fst t == name) doc.tables)
    pairs = (doc.root <> table "config") <#> \(Tuple k v) -> Tuple k (fromTomlValue v)
    build = table "buildInfo"
    lookupStr k = case snd <$> Array.find (\t -> fst t == k) build of
      Just (TString s) -> Just s
      _ -> Nothing
    buildInfo =
      if Array.null build then Nothing
      else Just { compiler: lookupStr "compiler", gccVersion: lookupStr "gccVersion", buildSalt: lookupStr "buildSalt" }
  pure (fromObject pairs) { buildInfo = buildInfo }
  where
  fromTomlValue = case _ of
    TString s -> Just (tristateOrText s)
    TInt n -> Just (Numeric n)
    TFloat f -> Just (maybe (Text (show f)) Numeric (Int.fromNumber f))
    TBool b -> Just (Flag b)

-- ---------------------------------------------------------------------------
-- Other text forms
-- ---------------------------------------------------------------------------

-- | A YAML document: `buildInfo` when known, then `config`, strings quoted.
toYaml :: KernelConfig -> String
toYaml config = joinWith "\n" $
  [ "---" ]
    <> maybe [] buildInfoLines config.buildInfo
    <> [ "", "config:" ]
    <> (flatEntries config.flatConfig <#> \(Tuple k v) -> "  " <> k <> ": " <> yamlValue v)
  where
  buildInfoLines b = [ "buildInfo:" ] <> catMaybes
    [ b.compiler <#> \c -> "  compiler: \"" <> c <> "\""
    , b.gccVersion <#> \g -> "  gccVersion: \"" <> g <> "\""
    , b.buildSalt <#> \s -> "  buildSalt: \"" <> s <> "\""
    ]
  yamlValue = case _ of
    Nothing -> "null"
    Just (Tristate t) -> "\"" <> show t <> "\""
    Just (Text s) -> "\"" <> s <> "\""
    Just (Numeric n) -> show n
    Just (Flag b) -> show b

-- | Makefile variables, with the `CONFIG_` prefix dropped.
toMakefile :: KernelConfig -> String
toMakefile config = joinWith "\n" $
  [ "# Kernel configuration as Makefile variables", "" ]
    <> (flatEntries config.flatConfig <#> line)
  where
  line (Tuple k v) =
    let
      name = fromMaybe k (stripPrefix (Pattern "CONFIG_") k)
    in
      case v of
        Nothing -> "# " <> k <> " is not set"
        Just value -> name <> " := " <> renderValue value

-- | Only the `y` and `m` options, as `.config` lines.
toEnabledOnly :: KernelConfig -> String
toEnabledOnly config = joinWith "\n" $ flatEntries config.flatConfig # mapMaybe case _ of
  Tuple k (Just (Tristate Y)) -> Just (k <> "=y")
  Tuple k (Just (Tristate M)) -> Just (k <> "=m")
  _ -> Nothing

-- | A bash script exporting every option.
toShellScript :: KernelConfig -> String
toShellScript config = joinWith "\n" $
  [ "#!/bin/bash", "# Kernel configuration as shell variables", "" ]
    <> (flatEntries config.flatConfig <#> line)
  where
  line (Tuple k v) = case v of
    Nothing -> "# " <> k <> " is not set"
    Just (Flag false) -> "# " <> k <> " is not set"
    Just (Text s) -> "export " <> k <> "=\"" <> s <> "\""
    Just value -> "export " <> k <> "=" <> renderValue value

-- | Options that differ, unified-diff style, old keys first.
toDiff :: KernelConfig -> KernelConfig -> String
toDiff old new = joinWith "\n" $
  [ "# Configuration differences", "" ] <> (keys >>= diffLines)
  where
  keys = nub (flatKeys old.flatConfig <> flatKeys new.flatConfig)
  diffLines k =
    let
      o = flatLookup k old.flatConfig
      n = flatLookup k new.flatConfig
    in
      if o == n then []
      else case o, n of
        Nothing, Just nv -> [ "+ " <> k <> "=" <> renderValue nv ]
        Just ov, Nothing -> [ "- " <> k <> "=" <> renderValue ov ]
        Just ov, Just nv -> [ "- " <> k <> "=" <> renderValue ov, "+ " <> k <> "=" <> renderValue nv ]
        Nothing, Nothing -> []
