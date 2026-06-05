use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::VfsError;

/// An entry returned by [`Filesystem::all_files`].
#[derive(Debug, Clone, PartialEq)]
pub struct FileEntry {
    /// Full path from the root, e.g. `"/src/main.rs"`.
    pub path: String,
    /// The file itself.
    pub file: VirtualFile,
}

/// A node in the virtual filesystem — either the root or a subdirectory.
///
/// Build the tree with [`Filesystem::build`], then query it with
/// [`find_file`] and [`all_files`].
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_virtual_filesystem::Filesystem;
///
/// let fs = Filesystem::build("/", |fs| {
///     fs.directory("src", |src| {
///         src.file("main.rs", Some("fn main() {}"), Default::default());
///     });
/// });
///
/// assert!(fs.find_file("src/main.rs").is_some());
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Filesystem {
    /// Name of this directory node.
    pub name: String,
    /// Child directories.
    pub children: Vec<Filesystem>,
    /// Files directly inside this directory.
    pub files: Vec<VirtualFile>,
    /// Arbitrary metadata attached to this node.
    pub attributes: HashMap<String, Value>,
}

impl Filesystem {
    /// Creates a new, empty filesystem node with the given name.
    ///
    /// Prefer [`build`] for a DSL-style initializer.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_virtual_filesystem::Filesystem;
    ///
    /// let fs = Filesystem::new("/");
    /// assert_eq!(fs.name, "/");
    /// ```
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            children: Vec::new(),
            files: Vec::new(),
            attributes: HashMap::new(),
        }
    }

    /// Creates a filesystem node and populates it via a closure.
    ///
    /// The closure receives a mutable reference to the node so nested
    /// calls to [`directory`] and [`file`] can compose the tree.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_virtual_filesystem::Filesystem;
    ///
    /// let fs = Filesystem::build("/", |root| {
    ///     root.directory("lib", |lib| {
    ///         lib.file("helper.rs", Some("// helper"), Default::default());
    ///     });
    /// });
    ///
    /// assert!(fs.find_file("lib/helper.rs").is_some());
    /// ```
    pub fn build(name: impl Into<String>, f: impl FnOnce(&mut Filesystem)) -> Self {
        let mut fs = Self::new(name);
        f(&mut fs);
        fs
    }

    /// Adds a child directory, configured by the supplied closure.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_virtual_filesystem::Filesystem;
    ///
    /// let mut fs = Filesystem::new("/");
    /// fs.directory("src", |_| {});
    /// assert_eq!(fs.children.len(), 1);
    /// ```
    pub fn directory(&mut self, name: impl Into<String>, f: impl FnOnce(&mut Filesystem)) {
        let mut dir = Filesystem::new(name);
        f(&mut dir);
        self.children.push(dir);
    }

    /// Adds a file to this directory node.
    ///
    /// Returns `Err` if the filename is invalid.
    ///
    /// # Errors
    ///
    /// Returns [`VfsError::InvalidName`] if `name` is empty, contains `/`,
    /// or contains a null byte.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_virtual_filesystem::Filesystem;
    ///
    /// let mut fs = Filesystem::new("/");
    /// fs.file("README.md", Some("# Hello"), Default::default()).unwrap();
    /// assert_eq!(fs.files.len(), 1);
    /// ```
    pub fn file(
        &mut self,
        name: impl Into<String>,
        content: Option<impl Into<String>>,
        attributes: HashMap<String, Value>,
    ) -> Result<&VirtualFile, VfsError> {
        let vf = VirtualFile::new(name, content, attributes)?;
        self.files.push(vf);
        Ok(self.files.last().unwrap())
    }

    /// Sets a metadata attribute on this node.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_virtual_filesystem::Filesystem;
    /// use serde_json::json;
    ///
    /// let mut fs = Filesystem::new("/");
    /// fs.set_attr("permissions", json!("0755"));
    /// assert_eq!(fs.get_attr("permissions").unwrap(), &json!("0755"));
    /// ```
    pub fn set_attr(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        self.attributes.insert(key.into(), value.into());
    }

    /// Gets a metadata attribute by key.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_virtual_filesystem::Filesystem;
    /// use serde_json::json;
    ///
    /// let mut fs = Filesystem::new("/");
    /// fs.set_attr("owner", json!("alice"));
    /// assert_eq!(fs.get_attr("owner").unwrap(), &json!("alice"));
    /// ```
    pub fn get_attr(&self, key: &str) -> Option<&Value> {
        self.attributes.get(key)
    }

    /// Finds a file by its relative path (e.g. `"src/main.rs"`).
    ///
    /// Returns `None` if any component of the path does not exist.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_virtual_filesystem::Filesystem;
    ///
    /// let fs = Filesystem::build("/", |root| {
    ///     root.directory("src", |src| {
    ///         src.file("lib.rs", Some(""), Default::default()).unwrap();
    ///     });
    /// });
    ///
    /// assert!(fs.find_file("src/lib.rs").is_some());
    /// assert!(fs.find_file("src/missing.rs").is_none());
    /// ```
    pub fn find_file(&self, path: &str) -> Option<&VirtualFile> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return None;
        }
        if parts.len() == 1 {
            return self.files.iter().find(|f| f.name == parts[0]);
        }
        let child = self.children.iter().find(|c| c.name == parts[0])?;
        child.find_file(&parts[1..].join("/"))
    }

    /// Returns all files in this subtree, each paired with its full path.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_virtual_filesystem::Filesystem;
    ///
    /// let fs = Filesystem::build("/", |root| {
    ///     root.file("a.txt", Some(""), Default::default()).unwrap();
    ///     root.directory("sub", |s| {
    ///         s.file("b.txt", Some(""), Default::default()).unwrap();
    ///     });
    /// });
    ///
    /// let all = fs.all_files("");
    /// assert_eq!(all.len(), 2);
    /// ```
    pub fn all_files(&self, prefix: &str) -> Vec<FileEntry> {
        let current = if prefix.is_empty() {
            if self.name == "/" {
                String::new()
            } else {
                format!("/{}", self.name)
            }
        } else {
            format!("{}/{}", prefix, self.name)
        };

        let mut result: Vec<FileEntry> = self
            .files
            .iter()
            .map(|f| FileEntry {
                path: if current.is_empty() {
                    format!("/{}", f.name)
                } else {
                    format!("{}/{}", current, f.name)
                },
                file: f.clone(),
            })
            .collect();

        for child in &self.children {
            result.extend(child.all_files(&current));
        }

        result
    }

    /// Renders a tree representation of the filesystem.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_virtual_filesystem::Filesystem;
    ///
    /// let fs = Filesystem::build("root", |r| {
    ///     r.file("a.txt", Some(""), Default::default()).unwrap();
    /// });
    ///
    /// let tree = fs.tree(0);
    /// assert!(tree.contains("root/"));
    /// assert!(tree.contains("a.txt"));
    /// ```
    pub fn tree(&self, indent: usize) -> String {
        let pad = " ".repeat(indent);
        let mut out = format!("{}{}/\n", pad, self.name);
        for f in &self.files {
            out.push_str(&format!("{}  {}\n", pad, f.name));
        }
        for child in &self.children {
            out.push_str(&child.tree(indent + 2));
        }
        out
    }
}

/// An immutable file inside a [`Filesystem`].
///
/// # Examples
///
/// ```rust
/// use std::collections::HashMap;
/// use cosmos_llm_virtual_filesystem::VirtualFile;
///
/// let f = VirtualFile::new("hello.txt", Some("hi"), HashMap::new()).unwrap();
/// assert_eq!(f.name, "hello.txt");
/// assert_eq!(f.content.as_deref(), Some("hi"));
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VirtualFile {
    /// Filename (no path separators).
    pub name: String,
    /// File content, if any.
    pub content: Option<String>,
    /// Arbitrary metadata.
    pub attributes: HashMap<String, Value>,
}

impl VirtualFile {
    /// Creates a new virtual file, validating the filename.
    ///
    /// # Errors
    ///
    /// Returns [`VfsError::InvalidName`] if `name` is empty, contains `/`,
    /// or contains a null byte.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::collections::HashMap;
    /// use cosmos_llm_virtual_filesystem::VirtualFile;
    ///
    /// let ok = VirtualFile::new("file.txt", Some("hello"), HashMap::new());
    /// assert!(ok.is_ok());
    ///
    /// let bad = VirtualFile::new("path/to/file", Some(""), HashMap::new());
    /// assert!(bad.is_err());
    /// ```
    pub fn new(
        name: impl Into<String>,
        content: Option<impl Into<String>>,
        attributes: HashMap<String, Value>,
    ) -> Result<Self, VfsError> {
        let name = name.into();
        if name.is_empty() {
            return Err(VfsError::InvalidName("filename cannot be empty".into()));
        }
        if name.contains('/') {
            return Err(VfsError::InvalidPath(
                "filename cannot contain path separators".into(),
            ));
        }
        if name.contains('\x00') {
            return Err(VfsError::InvalidPath(
                "filename cannot contain null bytes".into(),
            ));
        }
        Ok(Self {
            name,
            content: content.map(|c| c.into()),
            attributes,
        })
    }

    /// Returns a copy of this file with different content.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::collections::HashMap;
    /// use cosmos_llm_virtual_filesystem::VirtualFile;
    ///
    /// let f = VirtualFile::new("a.txt", Some("old"), HashMap::new()).unwrap();
    /// let f2 = f.with_content(Some("new"));
    /// assert_eq!(f2.content.as_deref(), Some("new"));
    /// ```
    pub fn with_content(&self, content: Option<impl Into<String>>) -> Self {
        Self {
            name: self.name.clone(),
            content: content.map(|c| c.into()),
            attributes: self.attributes.clone(),
        }
    }

    /// Returns a copy of this file with merged attributes.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::collections::HashMap;
    /// use cosmos_llm_virtual_filesystem::VirtualFile;
    /// use serde_json::json;
    ///
    /// let f = VirtualFile::new("a.txt", Some("hi"), HashMap::new()).unwrap();
    /// let f2 = f.with_attributes([("exec".to_string(), json!(true))].into());
    /// assert_eq!(f2.attributes["exec"], json!(true));
    /// ```
    pub fn with_attributes(&self, updates: HashMap<String, Value>) -> Self {
        let mut attrs = self.attributes.clone();
        attrs.extend(updates);
        Self {
            name: self.name.clone(),
            content: self.content.clone(),
            attributes: attrs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_and_find() {
        let fs = Filesystem::build("/", |root| {
            root.directory("src", |src| {
                src.file("main.rs", Some("fn main(){}"), Default::default())
                    .unwrap();
            });
        });

        let f = fs.find_file("src/main.rs").unwrap();
        assert_eq!(f.content.as_deref(), Some("fn main(){}"));
    }

    #[test]
    fn find_missing_returns_none() {
        let fs = Filesystem::new("/");
        assert!(fs.find_file("nope.txt").is_none());
    }

    #[test]
    fn all_files_collects_recursively() {
        let fs = Filesystem::build("/", |root| {
            root.file("a.txt", Some(""), Default::default()).unwrap();
            root.directory("sub", |s| {
                s.file("b.txt", Some(""), Default::default()).unwrap();
            });
        });
        let all = fs.all_files("");
        assert_eq!(all.len(), 2);
        let paths: Vec<_> = all.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"/a.txt"));
        assert!(paths.contains(&"/sub/b.txt"));
    }

    #[test]
    fn invalid_filename_rejected() {
        let r = VirtualFile::new("a/b.txt", Some(""), HashMap::new());
        assert!(r.is_err());

        let r2 = VirtualFile::new("", Some(""), HashMap::new());
        assert!(r2.is_err());
    }

    #[test]
    fn attrs() {
        let mut fs = Filesystem::new("/");
        fs.set_attr("mode", serde_json::json!("0755"));
        assert_eq!(fs.get_attr("mode").unwrap(), &serde_json::json!("0755"));
    }
}
