//! Hand-written argument parsing: global options anywhere on the line, then a subcommand and its
//! positional arguments and options.

use std::path::PathBuf;

use erpcraft_mcp::control::{DEFAULT_ADDR, normalize_addr};

/// Options that take a value (`--name value` or `--name=value`).
const VALUE_OPTIONS: &[&str] = &["data", "db", "filter", "search", "fields", "limit", "offset", "type", "workdate"];
/// Options that are on/off.
const FLAG_OPTIONS: &[&str] = &["json", "demo", "empty", "force", "all", "help", "version"];

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub data: Option<PathBuf>,
    /// `--db <postgres-url>`.
    pub db: Option<String>,
    /// `--connect [addr]`: drive the running desktop app (normalised `host:port`).
    pub connect: Option<String>,
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
        let mut it = args.into_iter().peekable();
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
            if name == "connect" {
                // The address is optional: `--connect`, `--connect 8000`, `--connect host:port`,
                // `--connect=addr`. A following word that isn't an address is left alone.
                let addr = match inline {
                    Some(v) => normalize_addr(&v).ok_or_else(|| format!("--connect needs host:port or a port, got '{v}'"))?,
                    None => match it.next_if(|n| normalize_addr(n).is_some()) {
                        Some(v) => normalize_addr(&v).unwrap_or_else(|| DEFAULT_ADDR.to_string()),
                        None => DEFAULT_ADDR.to_string(),
                    },
                };
                out.connect = Some(addr);
            } else if VALUE_OPTIONS.contains(&name.as_str()) {
                let value = match inline {
                    Some(v) => v,
                    None => it.next().ok_or_else(|| format!("option --{name} needs a value"))?,
                };
                match name.as_str() {
                    "data" => out.data = Some(PathBuf::from(value)),
                    "db" => out.db = Some(value),
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

    #[test]
    fn db_and_connect() {
        let a = parse(&["--db", "postgres://erp:secret@localhost/erp", "tables"]).unwrap();
        assert_eq!(a.db.as_deref(), Some("postgres://erp:secret@localhost/erp"));
        assert_eq!(a.positional, ["tables"]);
        assert_eq!(parse(&["--db=postgres://h/x", "tables"]).unwrap().db.as_deref(), Some("postgres://h/x"));
        assert!(parse(&["tables", "--db"]).is_err());

        let a = parse(&["--connect", "run", "company.info"]).unwrap();
        assert_eq!(a.connect.as_deref(), Some("127.0.0.1:7979"));
        assert_eq!(a.positional, ["run", "company.info"]);
        let a = parse(&["mcp", "--connect"]).unwrap();
        assert_eq!((a.connect.as_deref(), a.positional.as_slice()), (Some("127.0.0.1:7979"), &["mcp".to_string()][..]));
        assert_eq!(parse(&["--connect", "8000", "tables"]).unwrap().connect.as_deref(), Some("127.0.0.1:8000"));
        assert_eq!(parse(&["--connect", "localhost:9000", "tables"]).unwrap().connect.as_deref(), Some("localhost:9000"));
        assert_eq!(parse(&["--connect=7000", "tables"]).unwrap().connect.as_deref(), Some("127.0.0.1:7000"));
        assert!(parse(&["--connect=nowhere", "tables"]).is_err());
    }
}
