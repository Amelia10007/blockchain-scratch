use crate::Service;
use reqwest::blocking::{Client, ClientBuilder, Response};
use reqwest::Url;
use std::{marker::PhantomData, net::SocketAddr};
use warp::Filter;

pub struct HttpServer<S> {
    _phantom: PhantomData<fn() -> S>,
}

impl<S: Service> HttpServer<S> {
    pub async fn start<F>(addr: impl Into<SocketAddr>, handler: F)
    where
        F: Fn(S::Req) -> Option<S::Res> + Clone + Send + Sync + 'static,
    {
        let service = warp::path(S::NAME)
            .and(warp::body::json::<S::Req>())
            .map(move |req| {
                println!("DEBUG: request: {}", serde_json::to_string(&req).unwrap());
                let req = handler(req);
                req.as_ref()
                    .map(warp::reply::json)
                    .unwrap_or(warp::reply::json(&""))
            });

        warp::serve(service).run(addr).await;
    }
}

#[derive(Debug, Clone)]
pub struct DestinationCollection {
    sockets: Vec<SocketAddr>,
}

impl DestinationCollection {
    pub fn new(sockets: Vec<SocketAddr>) -> Self {
        Self { sockets }
    }
}

pub struct HttpClient<S> {
    destination: DestinationCollection,
    client: Client,
    _phantom: PhantomData<fn() -> S>,
}

impl<S: Service> HttpClient<S> {
    pub fn new(destination: DestinationCollection) -> Result<Self, ClientError> {
        let client = ClientBuilder::new().build()?;
        let httpclient = Self {
            destination,
            client,
            _phantom: PhantomData,
        };
        Ok(httpclient)
    }

    pub fn call(&self, req: &S::Req) -> Result<S::Res, ClientError> {
        let json = serde_json::to_string(req)?;

        for url in self.urls() {
            let req = self.client.get(url).body(json.clone()).build()?;
            if let Ok(res_text) = self.client.execute(req).and_then(Response::text) {
                if let Ok(res) = serde_json::from_str::<S::Res>(&res_text) {
                    return Ok(res);
                }
            }
        }

        Err(ClientError::NoResponse)
    }

    fn urls(&self) -> impl Iterator<Item = Url> + '_ {
        self.destination
            .sockets
            .iter()
            .map(|socket| {
                Url::parse(&format!("http://{}", socket.to_string()))
                    .and_then(|url| url.join(S::NAME))
            })
            .flatten()
    }
}

#[derive(Debug)]
pub enum ClientError {
    Serde(serde_json::Error),
    Request(reqwest::Error),
    NoResponse,
}

impl From<serde_json::Error> for ClientError {
    fn from(e: serde_json::Error) -> Self {
        ClientError::Serde(e)
    }
}

impl From<reqwest::Error> for ClientError {
    fn from(e: reqwest::Error) -> Self {
        ClientError::Request(e)
    }
}
