
use std::{
    cmp::max, ffi::OsStr, fmt::{Debug, Display}, fs, hash::Hash, ops::Range, path::PathBuf
};
use line_col::LineColLookup;
use colored::{Color, Colorize};

pub enum Underline {
    /// Error squiggle
    Squiggle,
    /// Highlight
    Highlight,
    /// Gray underline
    Normal,
}

impl Underline {
    fn line(&self, range: Range<usize>) -> String {
        let (symbol, color) = match self {
            Self::Squiggle => ("~", Color::Red),
            Self::Highlight => ("^", Color::Cyan),
            Self::Normal => ("-", Color::Black),
        };
        format!("{}{}",
            " ".repeat(range.start),
            symbol.repeat(max(1, range.end - range.start)).color(color)
        )
    }
}

#[derive(Debug)]
pub struct Span<'s>(pub &'s Src, pub Range<usize>);

impl Span<'_> {
    pub fn builtin() -> Self {
        Self(Src::builtin(), 0..0)
    }
    pub fn underlined(&self, style: Underline) -> String {
        // Get the starting and ending linecols as 0-based indices
        let sub_tuple = |a: (usize, usize)| { (a.0 - 1, a.1 - 1) };
        let lookup = LineColLookup::new(self.0.data());
        let start = sub_tuple(lookup.get(self.1.start));
        let end = sub_tuple(lookup.get(self.1.end));

        let mut lines = self.0
            .data().lines()
            .skip(start.0).take(end.0 - start.0 + 1);

        let padding = (end.0 + 1).to_string().len();
        let output_line = |line: usize, content, range| {
            format!(
                "{:pad1$}{}{}\n{:pad2$}{}\n",
                line.to_string().yellow(), " | ".black(), content,
                "", style.line(range),
                pad1 = padding - line.to_string().len(),
                pad2 = padding + 3
            )
        };
        
        let underlined = if end.0 == start.0 {
            output_line(start.0 + 1, lines.next().unwrap(), start.1..end.1)
        }
        else {
            let mut res = String::new();
            let mut i = 1;
            let len = end.0 - start.0;
            for line in lines {
                res.push_str(&output_line(start.0 + i, line, match i {
                    _ if i == len => 0..end.1,
                    1 => start.1..line.len(),
                    _ => 0..line.len(),
                }));
                i += 1;
            }
            res
        };
        format!(
            "{}{}{}\n{}",
            " ".repeat(padding), "--> ".black(), self.to_string().black(),
            underlined
        )
    }
}

impl Clone for Span<'_> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1.clone())
    }
}

impl Display for Span<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let lookup = LineColLookup::new(self.0.data());
        let start = lookup.get(self.1.start);
        if self.1.is_empty() {
            write!(f, "{}:{}:{}", self.0.name(), start.0, start.1)
        }
        else {
            let end = lookup.get(self.1.end);
            write!(f, "{}:{}:{}-{}:{}", self.0.name(), start.0, start.1, end.0, end.1)
        }
    }
}

/// A source file of code. Not necessarily a file, can also come from compiler 
/// built-ins
pub enum Src {
    Builtin,
    Memory {
        name: String,
        data: String,
    },
    File {
        path: PathBuf,
        data: String,
    }
}

impl Src {
    pub fn builtin<'s>() -> &'s Src {
        &Self::Builtin
    }
    pub fn from_memory<S: Into<String>, D: Into<String>>(name: S, data: D) -> Result<Src, String> {
        Ok(Src::Memory { name: name.into(), data: data.into() })
    }
    pub fn from_file<P: Into<PathBuf>>(path: P) -> Result<Src, String> {
        let path = path.into();
        Ok(Src::File {
            data: fs::read_to_string(&path).map_err(|e| format!("Can't read file: {}", e))?,
            path,
        })
    }
    pub fn name(&self) -> String {
        match self {
            Src::Builtin => String::from("<compiler built-in>"),
            Src::Memory { name, data: _ } => name.clone(),
            Src::File { path, data: _ } => path.to_string_lossy().to_string(),
        }
    }
    pub fn data(&self) -> &str {
        match self {
            Src::Builtin => "",
            Src::Memory { name: _, data } => data.as_str(),
            Src::File { path: _, data } => data.as_str(),
        }
    }
    pub fn cursor(&self) -> SrcCursor {
        SrcCursor(self, 0)
    }
}

impl Debug for Src {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Builtin => f.write_str("Builtin"),
            Self::Memory { name, data: _ } => f.write_fmt(format_args!("Memory({name:?})")),
            Self::File { path, data: _ } => f.write_fmt(format_args!("File({path:?})")),
        }
    }
}
impl Display for Src {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name())
    }
}

impl PartialEq for Src {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Src::Builtin, Src::Builtin) => true,
            (Src::File { path: a, data: _ }, Self::File { path: b, data: _ }) => a == b,
            (_, _) => false
        }
    }
}
impl Eq for Src {}
impl Hash for Src {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Src::Builtin => 0.hash(state),
            Src::Memory { name, data } => {
                name.hash(state);
                data.hash(state);
            },
            Src::File { path, data: _ } => path.hash(state),
        }
    }
}

pub struct SrcCursor<'s>(&'s Src, usize);
impl<'s> SrcCursor<'s> {
    pub fn next(&mut self) -> Option<char> {
        self.0.data()[self.1..].chars().next().inspect(|c| self.1 += c.len_utf8())
    }
    pub fn peek(&self) -> Option<char> {
        self.0.data()[self.1..].chars().next()
    }
    pub fn peek_n(&self, n: usize) -> Option<char> {
        self.0.data()[self.1..].chars().nth(n)
    }
    pub fn prev(&mut self) -> Option<char> {
        self.0.data()[..self.1].chars().next_back().inspect(|c| self.1 -= c.len_utf8())
    }
    pub fn peek_prev(&self) -> Option<char> {
        self.0.data()[..self.1].chars().next_back()
    }
    pub fn peek_prev_n(&self, n: usize) -> Option<char> {
        self.0.data()[..self.1].chars().nth_back(n)
    }
    pub fn position(&self) -> usize {
        self.1
    }
}
impl<'s> Iterator for SrcCursor<'s> {
    type Item = char;
    fn next(&mut self) -> Option<Self::Item> {
        self.next()
    }
}

// A pool of all the sources that are part of the same codebase
#[derive(Debug)]
pub struct SrcPool {
    srcs: Vec<Src>,
}

impl SrcPool {
    pub fn new(files: Vec<PathBuf>) -> Result<Self, String> {
        Ok(Self {
            srcs: files.into_iter().map(Src::from_file).collect::<Result<_, _>>()?
        })
    }
    pub fn new_from_dir(dir: PathBuf) -> Result<Self, String> {
        if dir.is_file() {
            return Self::new(vec![dir]);
        }
        if !dir.exists() {
            Err("Directory does not exist".to_string())?;
        }
        let srcs = Self::find_src_files(dir);
        if srcs.is_empty() {
            Err("Directory is empty".to_string())
        }
        else {
            Self::new(srcs)
        }
    }
    fn find_src_files(dir: PathBuf) -> Vec<PathBuf> {
        let mut res = vec![];
        if let Ok(entries) = std::fs::read_dir(dir) { 
            for entry in entries {
                let file = entry.unwrap();
                if let Ok(ty) = file.file_type() {
                    if ty.is_dir() {
                        res.extend(Self::find_src_files(file.path()));
                    }
                    else if file.path().extension() == Some(OsStr::new("dash")) {
                        res.push(file.path());
                    }
                }
            }
        }
        res
    }
    pub fn iter<'s>(&'s self) -> impl Iterator<Item = &'s Src> {
        self.into_iter()
    }
}

impl<'a> IntoIterator for &'a SrcPool {
    type IntoIter = <&'a Vec<Src> as IntoIterator>::IntoIter;
    type Item = &'a Src;
    fn into_iter(self) -> Self::IntoIter {
        self.srcs.iter()
    }
}