use std::collections::BTreeMap;
use regex::Regex;
use regex::Captures;
use lazy_static::lazy_static;

lazy_static! {
    static ref REPLACEMENT_REGEX: Regex = Regex::new(r"\$\{([^\}]*)\}").unwrap();
}

#[derive(Clone)]
pub struct Environment {
    map: BTreeMap<String, String>,
}

impl Environment {
    pub fn new() -> Environment {
        Environment {
            map: BTreeMap::new(),
        }
    }

    pub fn get(&self, variable: &str) -> Option<&String> {
        return self.map.get(variable);
    }

    pub fn set(&mut self, variable: &str, value: &str) {
        self.map.insert(String::from(variable), String::from(value));
    }

    pub fn remove(&mut self, variable: &str) {
        self.map.remove(variable);
    }

    pub fn resolve(&self, fmt_string: &str) -> String {
        return REPLACEMENT_REGEX.replace_all(fmt_string, |captures: &Captures| {
            match self.map.get(&captures[1]) {
                Some(s) => s,
                None => {
                    println!("Need to define '{0}'", &captures[1]);
                    ""
                },
            }
        }).into_owned();
    }
}
