use anyhow::{Context, Result, bail};
use chrono::Local;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// id 字母表: 小写字母+数字, 去掉易混淆的 0 o 1 l i, 保证双击能整段选中
const ID_ALPHABET: &[char] = &[
    '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'j', 'k', 'm',
    'n', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
];
const ID_LEN: usize = 6;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    pub id: String,
    pub content: String,
    pub created: String,
    pub updated: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SaveFile {
    prompts: Vec<Prompt>,
}

pub struct Store {
    file: PathBuf,
    prompts: Vec<Prompt>,
}

fn now_str() -> String {
    Local::now().format("%Y-%m-%d %H:%M").to_string()
}

fn data_file() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("PB_DATA_FILE") {
        let path = PathBuf::from(p);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).ok();
        }
        return Ok(path);
    }
    let dirs = ProjectDirs::from("", "", "prompt-board").context("无法定位用户数据目录")?;
    fs::create_dir_all(dirs.data_dir()).ok();
    Ok(dirs.data_dir().join("prompts.json"))
}

impl Store {
    pub fn load() -> Result<Self> {
        Self::open(data_file()?)
    }

    fn open(file: PathBuf) -> Result<Self> {
        if !file.exists() {
            return Ok(Self { file, prompts: Vec::new() });
        }
        let text =
            fs::read_to_string(&file).with_context(|| format!("读取数据文件失败: {}", file.display()))?;
        if text.trim().is_empty() {
            return Ok(Self { file, prompts: Vec::new() });
        }
        let sf: SaveFile = serde_json::from_str(&text).with_context(|| {
            format!(
                "数据文件损坏: {} , 请手动修复或删除后重新添加",
                file.display()
            )
        })?;
        Ok(Self { file, prompts: sf.prompts })
    }

    /// 原子写: 先落 .tmp 再 rename
    fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&SaveFile {
            prompts: self.prompts.clone(),
        })?;
        let tmp = self.file.with_extension("json.tmp");
        fs::write(&tmp, json).with_context(|| format!("写入 {}", tmp.display()))?;
        fs::rename(&tmp, &self.file).with_context(|| format!("移动到 {}", self.file.display()))?;
        Ok(())
    }

    pub fn list(&self) -> &[Prompt] {
        &self.prompts
    }

    /// 先精确匹配 id, 再按唯一前缀匹配
    fn index_of(&self, query: &str) -> Result<usize> {
        if let Some(i) = self.prompts.iter().position(|p| p.id == query) {
            return Ok(i);
        }
        let hits: Vec<usize> = self
            .prompts
            .iter()
            .enumerate()
            .filter(|(_, p)| p.id.starts_with(query))
            .map(|(i, _)| i)
            .collect();
        match hits.len() {
            0 => bail!("找不到提示词 `{query}` , 用 `pb ls` 查看全部"),
            1 => Ok(hits[0]),
            _ => {
                let ids: Vec<&str> = hits.iter().map(|&i| self.prompts[i].id.as_str()).collect();
                bail!("`{query}` 匹配到多个: {}", ids.join(" "))
            }
        }
    }

    pub fn get(&self, query: &str) -> Result<Prompt> {
        let i = self.index_of(query)?;
        Ok(self.prompts[i].clone())
    }

    fn gen_id(&self) -> String {
        loop {
            let id = nanoid::nanoid!(ID_LEN, ID_ALPHABET);
            if !self.prompts.iter().any(|p| p.id == id) {
                return id;
            }
        }
    }

    pub fn add(&mut self, content: &str) -> Result<Prompt> {
        let now = now_str();
        let p = Prompt {
            id: self.gen_id(),
            content: content.to_string(),
            created: now.clone(),
            updated: now,
        };
        self.prompts.push(p.clone());
        self.save()?;
        Ok(p)
    }

    pub fn update(&mut self, query: &str, content: &str) -> Result<Prompt> {
        let i = self.index_of(query)?;
        let mut changed = false;
        {
            let p = &mut self.prompts[i];
            if content != p.content {
                p.content = content.to_string();
                p.updated = now_str();
                changed = true;
            }
        }
        if changed {
            self.save()?;
        }
        Ok(self.prompts[i].clone())
    }

    pub fn remove(&mut self, query: &str) -> Result<Prompt> {
        let i = self.index_of(query)?;
        let p = self.prompts.remove(i);
        self.save()?;
        Ok(p)
    }

    /// 清空全部, 返回删除条数
    pub fn clear(&mut self) -> Result<usize> {
        let n = self.prompts.len();
        self.prompts.clear();
        self.save()?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> Store {
        let file = std::env::temp_dir().join(format!("pb-test-{name}.json"));
        let _ = fs::remove_file(&file);
        Store::open(file).unwrap()
    }

    #[test]
    fn add_get_prefix_remove() {
        let mut s = temp_store("crud");
        let p = s.add("把下面的文本翻译成英文").unwrap();
        assert_eq!(p.id.len(), ID_LEN);
        assert_eq!(s.get(&p.id[..3]).unwrap().id, p.id); // 前缀唯一匹配
        let removed = s.remove(&p.id).unwrap();
        assert_eq!(removed.content, "把下面的文本翻译成英文");
        assert!(s.list().is_empty());
        assert!(s.get("zzzzzz").is_err());
    }

    #[test]
    fn update_only_touches_updated_when_changed() {
        let mut s = temp_store("upd");
        let p = s.add("old").unwrap();
        let same = s.update(&p.id, "old").unwrap();
        assert_eq!(same.updated, p.updated);
        let diff = s.update(&p.id, "new").unwrap();
        assert_eq!(diff.content, "new");
    }

    #[test]
    fn persistence_roundtrip() {
        let mut s = temp_store("round");
        let p = s.add("内容").unwrap();
        let s2 = Store::open(s.file.clone()).unwrap();
        assert_eq!(s2.get(&p.id).unwrap().content, "内容");
    }

    #[test]
    fn clear_removes_everything() {
        let mut s = temp_store("clear");
        s.add("a").unwrap();
        s.add("b").unwrap();
        assert_eq!(s.clear().unwrap(), 2);
        assert!(s.list().is_empty());
        assert_eq!(s.clear().unwrap(), 0);
        let s2 = Store::open(s.file.clone()).unwrap();
        assert!(s2.list().is_empty());
    }

    #[test]
    fn id_avoids_confusable_chars() {
        let s = temp_store("alphabet");
        for _ in 0..50 {
            let id = s.gen_id();
            assert!(!id.contains(['0', 'o', '1', 'l', 'i']));
        }
    }
}
