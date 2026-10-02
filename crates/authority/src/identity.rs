use std::net::IpAddr;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum Name {
    Dns(String),
    Ip(IpAddr),
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Identity {
    names: Vec<Name>,
}

impl Identity {
    pub fn new(names: impl IntoIterator<Item = Name>) -> Self {
        let mut unique = Vec::new();

        for name in names {
            if !unique.contains(&name) {
                unique.push(name);
            }
        }

        Self { names: unique }
    }

    pub fn names(&self) -> &[Name] {
        &self.names
    }

    pub fn common_name(&self) -> String {
        match self.names.first() {
            Some(Name::Dns(name)) => name.clone(),
            Some(Name::Ip(address)) => address.to_string(),
            None => String::from("asura"),
        }
    }
}
