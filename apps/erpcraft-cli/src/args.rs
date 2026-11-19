//! Hand-written argument parsing: global options anywhere on the line, then a subcommand and its
//! positional arguments and options.

use std::path::PathBuf;

/// Options that take a value (`--name value` or `--name=value`).
const VALUE_OPTIONS: &[&str] = &["data", "filter", "search", "fields", "limit", "offset", "type", "workdate"];
/// Options that are on/off.
const FLAG_OPTIONS: &[&str] = &["json", "demo", "empty", "force", "all", "help", "version"];

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub data: Option<PathBuf>,
    pub json: bool,
    pub all: bool,
    pub help: bool,
    pub version: bool,
    pub workdate: Option<String>,
    /// The subcommand and its positional arguments.
    pub positional: Vec<String>,
    /// Value options other than the global ones, in order (`filter` may repeat).
    pub options: Vec<(String, String)>,
    pub flags: Vec<String>,
}

impl Args {
    pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Args, String> {
        let mut out = Args::default();
        let mut it = args.into_iter();
        let mut only_positional = false;
        while let Some(a) = it.next() {
            if only_positional || a == "-" || !a.starts_with('-') {
                out.positional.push(a);
                continue;
            }
            if a == "--" {
                only_positional = true;
                continue;
            }
            let (name, inline) = match a.strip_prefix("--") {
                Some(long) => match long.split_once('=') {
                    Some((n, v)) => (n.to_string(), Some(v.to_string())),
                    None => (long.to_string(), None),
                },
                None => match a.as_str() {
                    "-h" => ("help".to_string(), None),
                    "-V" => ("version".to_string(), None),
                    "-d" => ("data".to_string(), None),
                    "-f" => ("filter".to_string(), None),
                    "-s" => ("search".to_string(), None),
                    "-n" => ("limit".to_string(), None),
                    _ => return Err(format!("unknown option '{a}'")),
                },
            };
            if VALUE_OPTIONS.contains(&name.as_str()) {
                let value = match inline {
                    Some(v) => v,
                    None => it.next().ok_or_else(|| format!("option --{name} needs a value"))?,
                };
                match name.as_str() {
                    "data" => out.data = Some(PathBuf::from(value)),
                    "workdate" => out.workdate = Some(value),
                    _ => out.options.push((name, value)),
                }
            } else if FLAG_OPTIONS.contains(&name.as_str()) {
                if inline.is_some() {
                    return Err(format!("option --{name} takes no value"));
                }
                match name.as_str() {
                    "json" => out.json = true,
                    "all" => out.all = true,
                    "help" => out.help = true,
                    "version" => out.version = true,
                    _ => out.flags.push(name),
                }
            } else {
                return Err(format!("unknown option '--{name}'"));
            }
        }
        Ok(out)
    }

    pub fn flag(&self, name: &str) -> bool {
        self.flags.iter().any(|f| f == name)
    }

    /// The last value of an option.
    pub fn option(&self, name: &str) -> Option<&str> {
        self.options.iter().rev().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// Every value of a repeatable option.
    pub fn option_all(&self, name: &str) -> Vec<&str> {
        self.options.iter().filter(|(n, _)| n == name).map(|(_, v)| v.as_str()).collect()
    }

    /// Fails on options the subcommand doesn't take.
    pub fn only(&self, command: &str, options: &[&str], flags: &[&str]) -> Result<(), String> {
        if let Some((n, _)) = self.options.iter().find(|(n, _)| !options.contains(&n.as_str())) {
            return Err(format!("'{command}' does not take --{n}"));
        }
        if let Some(f) = self.flags.iter().find(|f| !flags.contains(&f.as_str())) {
            return Err(format!("'{command}' does not take --{f}"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn parse(s: &[&str]) -> Result<Args, String> {
        Args::parse(s.iter().map(|s| (*s).to_string()))
    }

    #[test]
    fn globals_anywhere_and_repeatable_options() {
        let a = parse(&["list", "--data", "x.json", "Customer", "--filter", "No.=10000|20000", "--filter=Name=*a*", "--json"]).unwrap();
        assert_eq!(a.data, Some(PathBuf::from("x.json")));
        assert!(a.json);
        assert_eq!(a.positional, ["list", "Customer"]);
        assert_eq!(a.option_all("filter"), ["No.=10000|20000", "Name=*a*"]);
        assert!(a.only("list", &["filter"], &[]).is_ok());
        assert!(a.only("get", &[], &[]).is_err());
    }

    #[test]
    fn errors_and_stdin_marker() {
        assert!(parse(&["list", "--bogus"]).is_err());
        assert!(parse(&["list", "--limit"]).is_err());
        assert!(parse(&["init", "--force=yes"]).is_err());
        assert_eq!(parse(&["run", "x", "-"]).unwrap().positional, ["run", "x", "-"]);
        assert_eq!(parse(&["search", "--", "--weird"]).unwrap().positional, ["search", "--weird"]);
    }
}
