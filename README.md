# rust-routerify-aws-lambda-quickstart
A quickstart for setting up an HTTP API in AWS Lambda / API Gateway using Rust v1.43 and Routerify v1.1.4.

## What?

Below is a description of the steps to route and serve HTTP requests from AWS API Gateway and AWS Lambda to a Rust application (using a Rust HTTP routing library).

### Why?

For most use cases, serving a web API with a traditional 24x7 hosting server _works_. However, there are use cases without specialized hardware requirements, which create an opportunity to reduce costs through the use of host-agnostic computing platforms (i.e. [Function-as-a-Service](https://en.wikipedia.org/wiki/Function_as_a_service) platforms like AWS Lambda).

### Why Rust?

Rust is an expressive, fast, and reliable language to use for building any applications, once a point in the learning curve has been reached... It has a number of key benefits over other languages for greenfield projects:

* Rust's type system allows for concise and expressive modeling of business domains and their invariants,
* C-level speeds can be achieved with a memory-safe implementation for all of your project's technical details, 
* and a fast start-up time along with low runtime memory overhead allows us to take the most advantage of AWS Lambda's pricing at scale.

## The Steps Described

Create the following `Cargo.toml`:

```toml
[package]
authors = ["Bob Smith <bob@example.com>"]
edition = "2018"
name = "<your-crate-name>"
version = "0.0.1"

[dependencies]
hyper = "0.13.6"
# This version is pinned as there are no official releases of the Rust runtime as of 17/10/2020.
lambda_http = { git = "https://github.com/awslabs/aws-lambda-rust-runtime/", rev = "c36409c5"}
rand = "0.7.3"
routerify = "1.1.4"
routerify-cors = "1.1"
serde = { version = "1.0", features = ["std", "derive"] }
serde_json = "1.0"
tokio = { version = "0.2", features = ["full"] }
url = { version = "2.1.1", features = ["serde"] }
```

Import all this important stuff:

```rust
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
```

Create an entrypoint function using the tokio async-runtime:

```rust
#[tokio::main]
async fn main() -> Result<(), Error> {
    lambda::run(handler(start)).await?;
    Ok(())
}
```

Create an alias for the type of async errors dealt with by Hyper and Routerify:

```rust
type Error = Box<dyn std::error::Error + Sync + Send + 'static>;
```

Define a handler entrypoint for the Lambda function. The Lambda function must be integrated with a resource in API Gateway, and therefore must receive a API Gateway response and return a API Gateway response,

```rust
async fn start(req: lambda_http::Request, _ctx: Context) -> Result<impl IntoResponse, Error> {
  ...
}
```

The function will:

1. Receive an API Gateway event when the function is invoked,

```rust
async fn start(req: lambda_http::Request, _ctx: Context) -> Result<impl IntoResponse, Error> {
    ...
}
```

2. Convert this event from a lambda_http::Request into a `hyper::Request`, the type expected by our routing library Routerify.

```rust
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
```

3. Process the request through our Routerify-based HTTP program,

```rust
    // Generate some random state and build the HTTP router.
    let router = router(State{ count: rand::thread_rng().gen::<u8>() });
    // Start a internal Routerify server with the above router.
    let serve = serve(router).await;
    // Send the request to the routerify server and return the response.
    let resp = Client::new().request(req).await.unwrap();
    // Shutdown the Routerify server.
    serve.shutdown();
```

4. Convert the result from an HTTP response (`hyper::Response`) into a API Gateway response (`lambda_http::Response`).

```rust
    // Convert the hyper::Response into a lambda_http::Response.
    let (parts, body) = resp.into_parts();
    let body_bytes = hyper::body::to_bytes(body).await?;
    let body = String::from_utf8(body_bytes.to_vec()).unwrap();
    Ok(lambda_http::Response::from_parts(parts, lambda_http::Body::from(body)))
```

### Is it really that simple?

Yes... _yes, it is_. 🤯

## What is Routerify?

[Routerify](https://github.com/routerify/routerify) is a modular implementation of an HTTP router.

Routerify's main features:

* 📡 Supports complex, parameterized routing logic with stateful handlers and middleware chains,
* 🚀 Has a performant implementation based on [hyper](https://github.com/hyperium/hyper) and performs routing using [`RegexSet`](https://docs.rs/regex/1.3.9/regex/struct.RegexSet.html),
* 🍗 Well documented with examples,


### The steps to create a Routerify server

Create a builder function for "building" your Routerify router:

```rust
fn router(state: State) -> Router<Body, Infallible> {
    // NOTE: We have not defined `get_count`, which is the function which handles requests at this endpoint.
    Router::builder().data(state).get("/data", get_count).build().unwrap()
}
```

Define a struct which defines what runtime-initialized state your application requires:

```rust
struct State {
    count: u8
}
```

Create an async handler function for the `GET /data` endpoint of our API. 

Note about the return type: this API always returns a Result, thus the error is marked as [Infallible](https://doc.rust-lang.org/beta/std/convert/enum.Infallible.html). However, Hyper's type definitions still require a Result type to be returned.

Use the appropriate HTTP status code and always return an `Ok(..)` response, where the body and headers are updated with the appropriate data.

```rust
async fn get_count(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    // Access the app state.
    let state = req.data::<State>().unwrap();
    Ok(Response::builder()
        .status(hyper::StatusCode::OK)
        .body(Body::from(format!("Count: {}", state.count))))
}
```

## The Glue Between AWS Lambda and Routerify

In our `start` entrypoint handler, we setup a server in the AWS Lambda instance with our Routerify server:

```rust
    // Generate some random state and build the HTTP router.
    let router = router(State{ count: rand::thread_rng().gen::<u8>() });

    // Start an internal Routerify server with the above router.
    let serve = serve(router).await;
```

This function `serve` will bind a `routerify::Router` to the instance's localhost:

```rust
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
```

We can then serve the request to the server's local address, await the response, then shutdown the Routerify server (as we only serve one request per AWS Lambda instance).

```rust
    // Prefix the local Routerify's address to the path of the incoming Lambda request.
    let uri = format!("http://{}{}", serve.addr(), parts.uri.path());
    parts.uri = hyper::Uri::from_str(uri.as_str()).unwrap();
    let req = hyper::Request::from_parts(parts, body);

    // Send the request to the routerify server and return the response.
    let resp = Client::new().request(req).await.unwrap();

    // Shutdown the Routerify server.
    serve.shutdown();
```


