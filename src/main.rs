//use uuid::Uuid;
use clap::Parser;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug, Parser)]
#[clap(about, version, long_about = None)]
struct Opt {
    /// The AWS Region.
    #[clap(short, long, default_value = "us-east-1")]
    region: String,

    /// The Neo4J Username.
    #[clap(short, long, default_value = "neo4j")]
    user: String,

    /// The Neo4J Password.
    #[clap(short, long, default_value = "password")]
    pass: String,

    /// The Neo4J URI.
    #[clap(short, long, default_value = "127.0.0.1:7687")]
    uri: String,

    /// Whether to display additional information.
    #[clap(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() {
    let Opt { region, verbose, user, pass, uri } = Opt::parse();

    if verbose {
        tracing_subscriber::fmt::init();
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
}
