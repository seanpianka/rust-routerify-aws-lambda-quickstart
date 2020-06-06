use hyper::{Client, Server, Request, Body, Response};
use lambda_http::{self, handler, lambda, IntoResponse};
use routerify::{Router, RouterService};
use std::{net::SocketAddr, str::FromStr};
use tokio::sync::oneshot;
use std::convert::Infallible;
use routerify::prelude::RequestExt;
use rand::Rng;

#[tokio::main]
async fn main() -> Result<(), AsyncError> {
    lambda::run(handler(start)).await?;
    Ok(())
}

type AsyncError = Box<dyn std::error::Error + Sync + Send + 'static>;

async fn start(req: lambda_http::Request) -> Result<impl IntoResponse, AsyncError> {
    // Generate some random state and build the HTTP router.
    let router = router(State{ count: rand::thread_rng().gen::<u8>() });

    // Start a internal Routerify server with the above router.
    let serve = serve(router).await;

    // Convert the lambda_http::Request into a hyper::Request.
    let (mut parts, body) = req.into_parts();
    let body = match body {
        lambda_http::Body::Empty => hyper::Body::empty(),
        lambda_http::Body::Text(t) => hyper::Body::from(t.into_bytes()),
        lambda_http::Body::Binary(b) => hyper::Body::from(b.clone()),
    };
    // Prefix the local Routerify's address to the path of the incoming Lambda request.
    let uri = format!("http://{}{}", serve.addr(), parts.uri.path());
    parts.uri = hyper::Uri::from_str(uri.as_str()).unwrap();
    let req = hyper::Request::from_parts(parts, body);

    // Send the request to the routerify server and return the response.
    let resp = Client::new().request(req).await.unwrap();

    // Shutdown the Routerify server.
    serve.shutdown();

    // Convert the hyper::Response into a lambda_http::Response.
    let (parts, body) = resp.into_parts();
    let body_bytes = hyper::body::to_bytes(body).await?;
    let body = String::from_utf8(body_bytes.to_vec()).unwrap();
    Ok(lambda_http::Response::from_parts(parts, lambda_http::Body::from(body)))
}

struct State {
    count: u8
}

fn router(state: State) -> Router<Body, Infallible> {
    Router::builder().data(state).get("/data", get_count).build().unwrap()
}

async fn get_count(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    // Access the app state.
    let state = req.data::<State>().unwrap();
    Ok(Response::new(Body::from(format!("Count: {}", state.count))))
}

pub struct Serve {
    addr: SocketAddr,
    tx: oneshot::Sender<()>,
}

impl Serve {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn shutdown(self) {
        self.tx.send(()).unwrap();
    }
}

pub async fn serve<B, E>(router: Router<B, E>) -> Serve
    where
        B: hyper::body::HttpBody + Send + Sync + Unpin + 'static,
        E: std::error::Error + Send + Sync + Unpin + 'static,
        <B as hyper::body::HttpBody>::Data: Send + Sync + 'static,
        <B as hyper::body::HttpBody>::Error: std::error::Error + Send + Sync + 'static,
{
    let service = RouterService::new(router).unwrap();
    let server = Server::bind(&([127, 0, 0, 1], 0).into()).serve(service);
    let addr = server.local_addr();

    let (tx, rx) = oneshot::channel::<()>();

    let graceful_server = server.with_graceful_shutdown(async {
        rx.await.unwrap();
    });

    tokio::spawn(async move {
        graceful_server.await.unwrap();
    });

    Serve { addr, tx }
}

