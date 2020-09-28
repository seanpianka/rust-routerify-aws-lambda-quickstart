# rust-routerify-aws-lambda-quickstart
A quickstart for setting up an HTTP API in AWS Lambda / API Gateway using Rust v1.43 and Routerify v1.1.4.

## What?

Below are a description of the steps to route and serve HTTP requests from AWS API Gateway and AWS Lambda to a Rust application (using a Rust HTTP routing library).

### The Steps

Import all this important stuff:

```rust
use hyper::{Client, Server, Request, Body, Response};
use lambda_http::{self, handler, lambda, IntoResponse};
use routerify::{Router, RouterService};
use std::{net::SocketAddr, str::FromStr};
use tokio::sync::oneshot;
use std::convert::Infallible;
use routerify::prelude::RequestExt;
use rand::Rng;
```

Create an entrypoint function that uses the tokio async runtime:

```rust
#[tokio::main]
async fn main() -> Result<(), AsyncError> {
    lambda::run(handler(start)).await?;
    Ok(())
}
```

Alias the type of async errors we will be dealing with in Hyper / Routerify:

```rust
type AsyncError = Box<dyn std::error::Error + Sync + Send + 'static>;
```

Define a handler entrypoint for the Lambda function. The Lambda function must be integrated with a resource in API Gateway, and therefore must receive a API Gateway response and return a API Gateway response,

```rust
async fn start(req: lambda_http::Request) -> Result<impl IntoResponse, AsyncError> {
  ...
}
```

The function will:

1. Receive an API Gateway event when the function is invoked,

```rust
async fn start(req: lambda_http::Request) -> Result<impl IntoResponse, AsyncError> {
```

2. Convert this event into HTTP request,

```rust
   // Convert the lambda_http::Request into a hyper::Request.
    let (mut parts, body) = req.into_parts();
    let body = match body {
        lambda_http::Body::Empty => hyper::Body::empty(),
        lambda_http::Body::Text(t) => hyper::Body::from(t.into_bytes()),
        lambda_http::Body::Binary(b) => hyper::Body::from(b),
```

3. Process the request through our Routerify-based HTTP program,

```rust
    // Generate some random "backend application" state and build the HTTP router.
    let router = router(State{ count: rand::thread_rng().gen::<u8>() });

    // Start a internal Routerify server with the above router.
    let serve = serve(router).await;
    
    ...
    
    // Prefix the local Routerify's address to the path of the incoming Lambda request.
    let uri = format!("http://{}{}", serve.addr(), parts.uri.path());
    parts.uri = hyper::Uri::from_str(uri.as_str()).unwrap();
    let req = hyper::Request::from_parts(parts, body);

    // Send the request to the routerify server and return the response.
    let resp = Client::new().request(req).await.unwrap();
    
    // Shutdown the Routerify server.
    serve.shutdown();
```

4. Convert the result from an HTTP response into a API Gateway response.

```rust
    // Convert the hyper::Response into a lambda_http::Response.
    let (parts, body) = resp.into_parts();
    let body_bytes = hyper::body::to_bytes(body).await?;
    let body = String::from_utf8(body_bytes.to_vec()).unwrap();
    Ok(lambda_http::Response::from_parts(parts, lambda_http::Body::from(body)))
```

### Is it really that simple?

Yes... yes, it is.

## What is Routerify?


### More Steps

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

Note about the return type: this API always returns a Result, thus the error is marked as [Infallible](https://doc.rust-lang.org/beta/std/convert/enum.Infallible.html). However, Hyper's type definitions still require a Result type to be returned. When a request must indicate success or failure, 

```rust
async fn get_count(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    // Access the app state.
    let state = req.data::<State>().unwrap();
    Ok(Response::builder()
        .status(hyper::StatusCode::OK)
        .body(Body::from(format!("Count: {}", state.count))))
}
```
