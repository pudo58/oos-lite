use std::fs;
use std::path::Path;
use glob::Pattern;

pub struct IgnoreRules {
    patterns: Vec<Pattern>,
}

impl IgnoreRules {
    /// Loads default ignore patterns plus optional `.oosignore` from root directory.
    pub fn load(root: &Path) -> Self {
        let mut rules = Self::default_rules();

        let oosignore_path = root.join(".oosignore");
        if oosignore_path.exists() {
            if let Ok(content) = fs::read_to_string(&oosignore_path) {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        continue;
                    }
                    rules.add_pattern(trimmed);
                }
            }
        }

        rules
    }

    pub fn default_rules() -> Self {
        let mut rules = Self {
            patterns: Vec::new(),
        };

        let defaults = [
            "~$*",
            "*.tmp",
            "*.temp",
            "*.crdownload",
            "Thumbs.db",
            "desktop.ini",
            ".DS_Store",
            "*.swp",
            "*.swo",
            ".git",
            ".git/**",
            "node_modules",
            "node_modules/**",
            "target",
            "target/**",
            ".oos-store",
            ".oos-store/**",
            ".oosignore",
        ];

        for p in defaults {
            rules.add_pattern(p);
        }

        rules
    }

    pub fn add_pattern(&mut self, pattern_str: &str) {
        let pattern_str = pattern_str.trim();
        if pattern_str.is_empty() {
            return;
        }

        let normalized = pattern_str.replace('\\', "/");
        let is_dir_pattern = normalized.ends_with('/');
        let clean = normalized.trim_end_matches('/').to_string();

        if let Ok(p) = Pattern::new(&clean) {
            self.patterns.push(p);
        }
        if is_dir_pattern || !clean.contains('.') {
            if let Ok(p) = Pattern::new(&format!("{}/**", clean)) {
                self.patterns.push(p);
            }
            if let Ok(p) = Pattern::new(&format!("**/{}/**", clean)) {
                self.patterns.push(p);
            }
        }
    }

    /// Checks if a relative path or its filename matches any ignore pattern.
    pub fn is_ignored(&self, rel_path: &Path) -> bool {
        let rel_str = rel_path.to_string_lossy().replace('\\', "/");
        let file_name = rel_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        for pat in &self.patterns {
            if pat.matches(&file_name) || pat.matches(&rel_str) {
                return true;
            }
            for comp in rel_path.components() {
                if let std::path::Component::Normal(c) = comp {
                    let s = c.to_string_lossy();
                    if pat.matches(&s) {
                        return true;
                    }
                }
            }
        }

        false
    }
}
