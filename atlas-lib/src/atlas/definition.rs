use std::fmt;

/// The cloud provider this resource belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Provider {
    Aws,
    Gcp,
    Azure,
    Hetzner,
    DigitalOcean,
    MsGraph,
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// A node in the property graph
/// - `name`: the resource type (e.g., "AWS::EC2::Instance", "compute.instances")
/// - `category`: the service group (e.g., "AWS::EC2", "compute")
/// - `id`: the unique resource identifier (e.g., "i-12345", "vpc-abc")
/// - `provider`: which cloud this came from
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub category: String,
    pub provider: Provider,
}

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({})", self.name, self.id)
    }
}

/// Edge types for the topology graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Edge {
    Contains,   // Hierarchical containment
    ConnectsTo, // Routing/Traffic flow
    DependsOn,  // Logical dependency
    Manages,    // Management relationship
}

impl fmt::Display for Edge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
