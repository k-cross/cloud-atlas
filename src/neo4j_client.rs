// dependencies
use neo4rs::Graph;
use futures::stream::*;

// system
use std::sync::Arc;

asnync fn setup_client(user: String, pass: String, uri: String) -> Result<Graph, Error> {
   //let id = Uuid::new_v4().to_string();
   let graph = Arc::new(Graph::new(&uri, user, pass).await.unwrap())
}