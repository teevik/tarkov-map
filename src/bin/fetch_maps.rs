use base64::{Engine, prelude::BASE64_STANDARD};
use reqwest::{Client, header};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct GitHubContents {
    content: String,
}

#[derive(Debug, Deserialize)]
struct FetchedTarkovMaps {
    maps: Vec<FetchedMap>,
}

#[derive(Debug, Deserialize)]
struct FetchedMap {
    key: String,
    projection: String,
    author: String,
    svgPath: String,
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    env_logger::init();
    color_eyre::install()?;

    let client = Client::new();

    // let a = client
    //     .get("https://api.github.com/repos/the-hideout/tarkov-dev/contents/src/data/maps.json")
    //     .send()
    //     .await?
    //     .text()
    //     .await?;
    // dbg!(a);

    let a = client
        .get("https://api.github.com/repos/the-hideout/tarkov-dev/contents/src/data/maps.json")
        .header(header::USER_AGENT, "tarkov-map")
        .send()
        .await?
        .text()
        .await?;
    dbg!(a);
    //     .json()
    //     .await?;

    // dbg!()

    // dbg!(&content.trim());

    // let json = BASE64_STANDARD.decode(content.trim())?;
    // dbg!(&json);
    // let json: Vec<FetchedTarkovMaps> = serde_json::from_slice(&json)?;

    // dbg!(json);

    Ok(())
}
