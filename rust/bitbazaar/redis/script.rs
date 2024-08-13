use redis::{cmd, Cmd, ToRedisArgs};
use sha1_smol::Sha1;

/// A lua script wrapper. Should be created once per script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedisScript {
    code: String,
    hash: String,
}

// Implement hash for RedisScript as its used in a HashSet in batching:
impl std::hash::Hash for RedisScript {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

impl RedisScript {
    /// Create the script object.
    /// Use `include_str!()` to allow the script to exist in its own file for highlighting etc.
    pub fn new(code: &str) -> RedisScript {
        let mut hash = Sha1::new();
        hash.update(code.as_bytes());
        RedisScript {
            code: code.to_string(),
            hash: hash.digest().to_string(),
        }
    }

    /// Create a new script invoker for an individual script call.
    pub fn invoker(&self) -> RedisScriptInvoker<'_> {
        RedisScriptInvoker {
            script: self,
            args: vec![],
            keys: vec![],
        }
    }

    /// The command to load the script to redis.
    pub(crate) fn load_cmd(&self) -> Cmd {
        let mut cmd = cmd("SCRIPT");
        cmd.arg("LOAD").arg(self.code.as_bytes());
        cmd
    }
}

/// Represents a individual script call with specific args and keys.
pub struct RedisScriptInvoker<'a> {
    pub(crate) script: &'a RedisScript,
    args: Vec<Vec<u8>>,
    keys: Vec<Vec<u8>>,
}

impl<'a> RedisScriptInvoker<'a> {
    /// Add a regular argument. I.e. `ARGV[i]`
    #[inline]
    pub fn arg<'b, T: ToRedisArgs>(mut self, arg: T) -> Self
    where
        'a: 'b,
    {
        arg.write_redis_args(&mut self.args);
        self
    }

    /// Add a key argument. I.e. `KEYS[i]`
    #[inline]
    pub fn key<'b, T: ToRedisArgs>(mut self, key: T) -> Self
    where
        'a: 'b,
    {
        key.write_redis_args(&mut self.keys);
        self
    }

    /// The command to run the script invocation. Will fail if script not loaded yet.
    pub(crate) fn eval_cmd(&self) -> Cmd {
        let mut cmd = cmd("EVALSHA");
        cmd.arg(self.script.hash.as_bytes())
            .arg(self.keys.len())
            .arg(&*self.keys)
            .arg(&*self.args);
        cmd
    }
}
