use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct Config {
    lock: RwLock<Arc<Users>>,
}

impl Config {
    pub fn new(users: Users) -> Self {
        Self {
            lock: RwLock::new(Arc::new(users)),
        }
    }

    pub fn new_parsed(toml: &str) -> Result<Self, toml::de::Error> {
        Ok(Self::new(toml::from_str(toml)?))
    }

    pub async fn get(&self) -> Arc<Users> {
        self.lock.read().await.clone()
    }

    pub async fn set(&self, users: Users) {
        *self.lock.write().await = Arc::new(users);
    }

    pub async fn load(&self, toml: &str) -> Result<(), toml::de::Error> {
        self.set(toml::from_str(toml)?).await;
        Ok(())
    }
}

impl From<Users> for Config {
    fn from(value: Users) -> Self {
        Self {
            lock: RwLock::new(Arc::new(value)),
        }
    }
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Users {
    /// If true (default), the server allows enumerating users
    ///
    /// When this property is true, users can be individually unlisted with [User::unlisted].
    #[serde(default = "value::r#true")]
    pub enable_index: bool,

    #[serde(deserialize_with = "deserialize_users")]
    pub users: HashMap<String, User>,
}

impl Users {
    pub fn find(&self, name: &str) -> Option<&User> {
        self.users.get(name)
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct User {
    /// Automatically convert newlines in `info` and `long_info` to CRLF, and add an ending CRLF if one is missing
    #[serde(default = "value::r#true")]
    pub fix_crlf: bool,

    /// Plain text returned when querying this user
    pub info: Option<String>,

    /// Plain text returned when querying this user in verbose mode
    pub long_info: Option<String>,

    /// If true, this user won't be enumerated when a listing is requested
    #[serde(default)]
    pub unlisted: bool,
}

impl User {
    pub fn from_info(info: String) -> Self {
        Self {
            fix_crlf: true,
            info: Some(info),
            long_info: None,
            unlisted: false,
        }
    }

    pub fn info(&self) -> &str {
        match self {
            Self {
                info: Some(info), ..
            } => info,
            Self {
                long_info: Some(long_info),
                ..
            } => long_info,
            Self {
                long_info: None,
                info: None,
                ..
            } => "",
        }
    }

    pub fn long_info(&self) -> &str {
        match self {
            Self {
                long_info: Some(long_info),
                ..
            } => long_info,
            Self {
                info: Some(info), ..
            } => info,
            Self {
                long_info: None,
                info: None,
                ..
            } => "",
        }
    }

    /// Try to replace single LF with CRLF, and add a final CRLF, for each info text
    pub fn fix_crlf(&mut self) {
        if self.fix_crlf {
            if let Some(info) = &mut self.info {
                fix_string_crlf(info);
            }

            if let Some(long_info) = &mut self.long_info {
                fix_string_crlf(long_info);
            }
        }
    }
}

fn deserialize_users<'de, D: Deserializer<'de>>(de: D) -> Result<HashMap<String, User>, D::Error> {
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Either {
        String(String),
        User(User),
    }

    HashMap::<String, Either>::deserialize(de).map(|hm| {
        hm.into_iter()
            .map(|(key, value)| {
                let mut user = match value {
                    Either::String(info) => User::from_info(info),
                    Either::User(user) => user,
                };

                user.fix_crlf();

                (key, user)
            })
            .collect()
    })
}

fn fix_str_crlf(str: &str) -> String {
    str.lines().flat_map(|line| [line, "\r\n"]).collect()
}

fn fix_string_crlf(string: &mut String) {
    match string.split_once('\n') {
        None => string.push_str("\r\n"),
        Some((prev, "")) if !prev.ends_with('\r') => {
            string.truncate(string.len() - 1);
            string.push_str("\r\n");
        }
        Some((_, _)) => *string = fix_str_crlf(string),
    }
}

mod value {
    pub fn r#true() -> bool {
        true
    }
}
