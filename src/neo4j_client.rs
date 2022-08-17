pub mod graph_client {
  // dependencies
  use neo4rs::Graph;
  use futures::stream::*;
  //use std::sync::atomic::{AtomicU32, Ordering};
  //use uuid::Uuid;
  
  // system
  use std::sync::Arc;

  pub async fn setup_client(user: String, pass: String, uri: String) -> Result<Arc<Graph>, neo4rs::Error> {
     let graph = Arc::new(Graph::new(uri.as_str(), user.as_str(), pass.as_str()).await.unwrap());
     Ok(graph)
  }
}

   /*
   let mut result = graph.run(
     query("CREATE (p:Person {id: $id})").param("id", id.clone())
   ).await.unwrap();

   let mut handles = Vec::new();
   let mut count = Arc::new(AtomicU32::new(0));
   for _ in 1..=42 {
       let graph = graph.clone();
       let id = id.clone();
       let count = count.clone();
       let handle = tokio::spawn(async move {
           let mut result = graph.execute(
             query("MATCH (p:Person {id: $id}) RETURN p").param("id", id)
           ).await.unwrap();
           while let Ok(Some(row)) = result.next().await {
               count.fetch_add(1, Ordering::Relaxed);
           }
       });
       handles.push(handle);
   }

   futures::future::join_all(handles).await;
   assert_eq!(count.load(Ordering::Relaxed), 42);
   */
