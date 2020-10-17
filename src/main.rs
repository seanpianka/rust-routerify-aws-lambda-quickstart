use hyper::{Client, Server};
use lambda_http::{
    handler,
    lambda::{self, Context},
    Body, IntoResponse, Request, RequestExt, Response,
};
use rand::Rng;
use routerify::{Router, RouterService};
use std::convert::Infallible;
use std::{net::SocketAddr, str::FromStr};
use tokio::sync::oneshot;
use url;

const SERVER_ADDR: &str = "127.0.0.1:8080";

#[tokio::main]
async fn main() -> Result<(), Error> {
    lambda::run(handler(start)).await?;
    Ok(())
}

type Error = Box<dyn std::error::Error + Sync + Send + 'static>;

async fn start(req: lambda_http::Request, _ctx: Context) -> Result<impl IntoResponse, Error> {
    // Store a copy of the query parameters, since AWS Lambda parsed these already.
    let query_params = req.query_string_parameters();
    // Convert the lambda_http::Request into a hyper::Request.
    let (mut parts, body) = req.into_parts();
    let body = match body {
        lambda_http::Body::Empty => hyper::Body::empty(),
        lambda_http::Body::Text(t) => hyper::Body::from(t.into_bytes()),
        lambda_http::Body::Binary(b) => hyper::Body::from(b),
    };
    // Prefix the local Routerify server's address to the path of the incoming Lambda request.
    let mut uri = format!("http://{}{}", SERVER_ADDR, parts.uri.path());
    // AWS Lambda Rust Runtime will automatically parse the query params *and* remove those
    // query parameters from the original URI. This is fine if you're writing your logic directly
    // in the handler function, but for passing-through to a separate router library, we need to
    // re-url-encode the query parameters and place them back into the URI.
    if !query_params.is_empty() {
        uri += "?";
        // Create a peekable iterator over the query parameters. This is used to add "&" in between
        // each of the query parameters, but prevents adding an extraneous "&" at the end of the
        // query parameter string.
        let mut params = query_params.iter().peekable();
        while let Some((key, value)) = params.next() {
            uri += url::form_urlencoded::Serializer::new(String::new())
                .append_pair(key, value)
                .finish()
                .as_str();
            // If this is not the last parameter, append a "&" for the next parameter...
            if params.peek().is_some() {
                uri += "&";
            }
        }
    }
    parts.uri = match hyper::Uri::from_str(uri.as_str()) {
        Ok(uri) => uri,
        Err(e) => panic!(format!("failed to build uri: {:?}", e)),
    };
    let req = hyper::Request::from_parts(parts, body);

    // Generate some random state and build the HTTP router.
    let router = router(State{ count: rand::thread_rng().gen::<u8>() });
    // Start a internal Routerify server with the above router.
    let serve = serve(router).await;
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
    let server = Server::bind(&SocketAddr::from_str(SERVER_ADDR).unwrap()).serve(service);
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

