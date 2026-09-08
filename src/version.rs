use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreRelease {
    pub label: String,
    pub number: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemVer {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub pre: Option<PreRelease>,
}

impl SemVer {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self { major, minor, patch, pre: None }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.strip_prefix('v').unwrap_or(s);
        let (version_part, pre_part) = match s.split_once('-') {
            Some((v, p)) => (v, Some(p)),
            None => (s, None),
        };
        let parts: Vec<&str> = version_part.splitn(3, '.').collect();
        if parts.len() != 3 { return None; }
        let major = parts[0].parse().ok()?;
        let minor = parts[1].parse().ok()?;
        let patch = parts[2].parse().ok()?;
        let pre = match pre_part {
            Some(p) => {
                let pre_parts: Vec<&str> = p.splitn(2, '.').collect();
                if pre_parts.len() != 2 { return None; }
                let label = pre_parts[0];
                if label.is_empty() { return None; }
                let num_str = pre_parts[1];
                if num_str.len() > 1 && num_str.starts_with('0') { return None; }
                let number = num_str.parse().ok()?;
                Some(PreRelease { label: label.to_string(), number })
            }
            None => None,
        };
        Some(Self { major, minor, patch, pre })
    }

    pub fn bump_major(&self) -> Self { Self::new(self.major + 1, 0, 0) }
    pub fn bump_minor(&self) -> Self { Self::new(self.major, self.minor + 1, 0) }
    pub fn bump_patch(&self) -> Self { Self::new(self.major, self.minor, self.patch + 1) }

    pub fn with_rc(&self, number: u32) -> Self {
        Self {
            major: self.major, minor: self.minor, patch: self.patch,
            pre: Some(PreRelease { label: "rc".to_string(), number }),
        }
    }

    pub fn bump_rc(&self) -> Self {
        match &self.pre {
            Some(pre) => Self {
                major: self.major, minor: self.minor, patch: self.patch,
                pre: Some(PreRelease { label: pre.label.clone(), number: pre.number + 1 }),
            },
            None => self.with_rc(1),
        }
    }

    pub fn to_release(&self) -> Self { Self::new(self.major, self.minor, self.patch) }
    pub fn is_pre_release(&self) -> bool { self.pre.is_some() }
    pub fn is_rc(&self) -> bool { self.pre.as_ref().is_some_and(|p| p.label == "rc") }
    pub fn tag_name(&self) -> String { format!("v{self}") }
    /// Every spelling `parse` reads back as this version. gflow writes the
    /// first; a repo tagged by something else may only carry the second.
    pub fn tag_names(&self) -> [String; 2] { [self.tag_name(), self.to_string()] }
    pub fn release_branch(&self) -> String { format!("release/{}", self.to_release()) }
    pub fn hotfix_branch(&self) -> String { format!("hotfix/{}", self.to_release()) }
    pub fn release_fix_branch(&self, name: &str) -> String { format!("release-fix/{}/{name}", self.to_release()) }
    pub fn hotfix_fix_branch(&self, name: &str) -> String { format!("hotfix-fix/{}/{name}", self.to_release()) }
    pub fn release_chore_branch(&self, name: &str) -> String { format!("release-chore/{}/{name}", self.to_release()) }
}

impl Ord for SemVer {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.major.cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
            .then(match (&self.pre, &other.pre) {
                (None, None) => std::cmp::Ordering::Equal,
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(a), Some(b)) => a.label.cmp(&b.label).then(a.number.cmp(&b.number)),
            })
    }
}

impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
}

impl fmt::Display for SemVer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(pre) = &self.pre {
            write!(f, "-{}.{}", pre.label, pre.number)?;
        }
        Ok(())
    }
}

/// Head branch for one protected landing leg: `finish/<source>-into-<target>`
/// with slashes flattened. Cut from the source, merged into exactly one
/// target, deleted after landing — conflict resolution happens here so the
/// release/hotfix branch itself is never touched.
pub fn finish_branch_name(source: &str, target: &str) -> String {
    format!("finish/{}-into-{}", source.replace('/', "-"), target.replace('/', "-"))
}

/// Inverse of `finish_branch_name`, recovering the source branch. Finish
/// branches are only ever built from release/hotfix sources, whose flattened
/// form (`release-1.2.0`) is unambiguous: a SemVer never contains `-into-`.
pub fn finish_branch_source(branch: &str) -> Option<String> {
    let flattened = branch.strip_prefix("finish/")?;
    for kind in ["release", "hotfix"] {
        if let Some(rest) = flattened.strip_prefix(kind).and_then(|r| r.strip_prefix('-')) {
            if let Some((version, target)) = rest.split_once("-into-") {
                if SemVer::parse(version).is_some() && !target.is_empty() {
                    return Some(format!("{kind}/{version}"));
                }
            }
        }
    }
    None
}
