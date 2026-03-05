@0xf45e3c79c13b2d18;

struct UserSetupConfig {
  # Hash of the text-version EDN/JSON, for synchronization
  textHash @0 :Text;

  # Where does the user store their specific overrides?
  userConfigPath @1 :Text; # e.g. ".mentci/user.json"

  # What are the names of the environment variables we care about?
  # This avoids hardcoding any env var knowledge in the rust binary.
  # If a required env var is missing from userConfig, it can warn or use defaults.
  requiredEnvVars @2 :List(EnvVarReq);
}

struct EnvVarReq {
  name @0 :Text;
  # Optional default resolution method if user doesn't specify one
  defaultMethod @1 :Text;
  defaultPath @2 :Text;
}
