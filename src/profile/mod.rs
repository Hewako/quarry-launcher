use serde::{Deserialize, Serialize};
use std::path::PathBuf;

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub mc_version: String,
    #[serde(default)]
    pub loader: Option<String>,
}

pub struct ProfileStore {
    root: PathBuf,
}

impl ProfileStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn profiles_dir(&self) -> PathBuf {
        self.root.join("profiles")
    }
    pub fn profile_dir(&self, id: &str) -> PathBuf {
        self.profiles_dir().join(id)
    }
    pub fn game_dir(&self, id: &str) -> PathBuf {
        self.profile_dir(id).join("game")
    }
    pub fn list(&self) -> Result<Vec<Profile>, BoxErr> {
        let dir = self.profiles_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let toml_path = entry.path().join("profile.toml");
            if !toml_path.exists() {
                continue;
            }
            let content = std::fs::read_to_string(&toml_path)?;
            out.push(toml::from_str(&content)?);
        }
        out.sort_by(|a: &Profile, b| a.name.cmp(&b.name));
        Ok(out)
    }
    pub fn get(&self, id: &str) -> Result<Profile, BoxErr> {
        let path = self.profile_dir(id).join("profile.toml");
        if !path.exists() {
            return Err(format!("Профиль '{}' не найден", id).into());
        }
        Ok(toml::from_str(&std::fs::read_to_string(&path)?)?)
    }
    pub fn create(&self, name: &str, mc_version: &str, loader: Option<&str>) -> Result<Profile, BoxErr> {
        let id = slugify(name);
        if id.is_empty() {
            return Err("Имя профиля не может быть пустым".into());
        }
        let dir = self.profile_dir(&id);
        if dir.exists() {
            return Err(format!("Профиль '{}' уже существует", id).into());
        }
        std::fs::create_dir_all(dir.join("game"))?;
        let profile = Profile {
            id: id.clone(),
            name: name.to_string(),
            mc_version: mc_version.to_string(),
            loader: loader.map(|l| l.to_string()),
        };
        std::fs::write(dir.join("profile.toml"), toml::to_string_pretty(&profile)?)?;
        Ok(profile)
    }
    pub fn delete(&self, id: &str) -> Result<(), BoxErr> {
        let dir = self.profile_dir(id);
        if !dir.exists() {
            return Err(format!("Профиль '{}' не найден", id).into());
        }
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
}

fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in name.chars().flat_map(|c| c.to_lowercase()) {
        if c.is_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("My Survival"), "my-survival");
        assert_eq!(slugify("  Test 123  "), "test-123");
        assert_eq!(slugify("Русский мир"), "русский-мир");
        assert_eq!(slugify("!!!"), "");
    }
}
