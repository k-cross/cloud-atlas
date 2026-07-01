use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Node {
    Region { name: String },
    Vpc { id: String },
    Subnet { id: String },
    Instance { id: String },
    IpAddress { ip: String },
    Az { name: String },
    Tag { key: String, value: String },
    Generic { id: String },
}

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Node::Region { name } => write!(f, "Region({})", name),
            Node::Vpc { id } => write!(f, "Vpc({})", id),
            Node::Subnet { id } => write!(f, "Subnet({})", id),
            Node::Instance { id } => write!(f, "Instance({})", id),
            Node::IpAddress { ip } => write!(f, "IpAddress({})", ip),
            Node::Az { name } => write!(f, "Az({})", name),
            Node::Tag { key, value } => write!(f, "Tag({}={})", key, value),
            Node::Generic { id } => write!(f, "{}", id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Edge {
    Contains,
    AttachedTo,
    HasIp,
    HasTag,
    Generic,
}

impl fmt::Display for Edge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Edge::Contains => write!(f, "Contains"),
            Edge::AttachedTo => write!(f, "AttachedTo"),
            Edge::HasIp => write!(f, "HasIp"),
            Edge::HasTag => write!(f, "HasTag"),
            Edge::Generic => write!(f, ""),
        }
    }
}
