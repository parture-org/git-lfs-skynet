use std::io::BufRead;
use std::str::FromStr;

use actix::prelude::*;
use futures::{future, prelude::*, stream};

use crate::error::CliError;
use git_lfs_ipfs_lib::{
    ipfs,
    spec::{self, transfer::custom},
};

#[derive(Debug, Clone)]
struct Input(custom::Event);

impl Message for Input {
    type Result = Result<Response, CliError>;
}

#[derive(Debug, Clone)]
enum Response {
    Replay, // 1st download request waits until ls response is received, then the response for that is sent back, system termination will still require that a response come back later (?)
    // Streaming(Box<dyn Stream<Item = custom::Event, Error = ()>>),
    Now(custom::Event),
}

impl Message for Response {
    type Result = Result<(), ()>;
}

pub struct Transfer {
    engine: Option<actix::Addr<Engine>>,
}

impl Default for Transfer {
    fn default() -> Self {
        Transfer { engine: None }
    }
}

impl Actor for Transfer {
    type Context = Context<Self>;
    fn started(&mut self, ctx: &mut <Self as Actor>::Context) {
        let mut read_it =
            std::io::BufReader::new(std::io::stdin())
                .lines()
                .map(|r| -> Result<Input, CliError> {
                    r.map_err(CliError::Io).and_then(|buf| {
                        serde_json::from_str(&buf)
                            .map(Input)
                            .map_err(CliError::SerdeJsonError)
                    })
                });

        ctx.add_stream(stream::poll_fn(move || -> Poll<Option<Input>, CliError> {
            read_it.next().transpose().map(|x| Async::Ready(x))
        }));
    }
}

impl StreamHandler<Input, CliError> for Transfer {
    fn handle(&mut self, event: Input, ctx: &mut <Self as Actor>::Context) {
        match (self.engine.clone(), event) {
            (None, Input(custom::Event::Init(init))) => {
                self.engine = Some(Engine::new(ctx.address(), init).start());
                println!("{{}}");
            }
            (None, event) => {
                panic!(CliError::UnexpectedEvent(event.0));
            }
            (Some(_), Input(custom::Event::Init(init))) => {
                panic!(CliError::UnexpectedEvent(custom::Event::Init(init)));
            }
            (Some(_), Input(custom::Event::Terminate)) => {
                debug!("Stopping system");
                System::current().stop();
            }
            (Some(engine), event) => {
                debug!("Sending event {:?}", event);
                ctx.wait(actix::fut::wrap_future(engine.send(event.clone())).then(
                    move |res, actor: &mut Self, ctx| match res.unwrap() {
                        Ok(response) => {
                            debug!("Received response {:?}", response);
                            match response {
                                Response::Now(event) => {
                                    println!(
                                        "{}",
                                        serde_json::to_string(&event)
                                            .expect("Failed to serialize an event")
                                    );
                                }
                                Response::Replay => {
                                    StreamHandler::<Input, CliError>::handle(actor, event, ctx);
                                }
                            }
                            actix::fut::ok(())
                        }
                        Err(err) => {
                            panic!("{:?}", err);
                            actix::fut::err(())
                        }
                    },
                ));
            }
        };
    }

    fn error(&mut self, err: CliError, ctx: &mut Context<Self>) -> Running {
        panic!("{:?}", err);
    }
}

impl Handler<Response> for Transfer {
    type Result = <Response as Message>::Result;

    fn handle(&mut self, res: Response, ctx: &mut <Self as Actor>::Context) -> Self::Result {
        match res {
            Response::Now(event) => {
                println!(
                    "{}",
                    serde_json::to_string(&event).expect("Failed to serialize an event")
                );
            }
            _ => {}
        }
        Ok(())
    }
}

struct Engine {
    transfer: actix::Addr<Transfer>,
    init: custom::Init,
}

impl Engine {
    fn new(transfer: actix::Addr<Transfer>, init: custom::Init) -> Self {
        Self { transfer, init }
    }
}

impl Actor for Engine {
    type Context = Context<Self>;
}

impl Handler<Input> for Engine {
    type Result = ResponseActFuture<Self, Response, CliError>;
    fn handle(&mut self, event: Input, ctx: &mut <Self as Actor>::Context) -> Self::Result {
        match (event.0, &self.init.operation) {
            (custom::Event::Download(download), custom::Operation::Download) => {
                let cid = ipfs::sha256_to_cid(&download.object.oid).wait().ok();
                if let Some(cid) = cid {
                    let oid = download.object.oid.clone();
                    let mut output = std::env::current_dir().unwrap();
                    output.push(&download.object.oid);
                    Box::new(
                        actix::fut::wrap_stream(
                            ipfs::cat_to_fs(spec::ipfs::Path::ipfs(cid.clone()), output.clone())
                                .map_err(CliError::IpfsApiError),
                        )
                        .fold(0, move |mut bytes_so_far, x, actor: &mut Self, ctx| {
                            bytes_so_far += x;
                            println!(
                                "{}",
                                serde_json::to_string(&custom::Event::Progress(custom::Progress {
                                    oid: oid.clone(),
                                    bytes_so_far,
                                    bytes_since_last: x,
                                }))
                                .expect("Failed to serialize an event")
                            );
                            // TODO: Don't disobey actix style and just print events here, there must be a better way...
                            // ctx.spawn(actix::fut::wrap_future(actor.transfer.send(
                            //     Response::Now(custom::Event::Progress(custom::Progress {
                            //         oid: oid.clone(),
                            //         bytes_so_far,
                            //         bytes_since_last: x,
                            //     })),
                            // ).then(|_| {
                            //     future::ok(())
                            // })));
                            actix::fut::ok(bytes_so_far)
                        })
                        .map(move |_, _, _| {
                            Response::Now(custom::Event::Complete(custom::Complete {
                                oid: download.object.oid.clone(),
                                error: None,
                                path: Some(output),
                            }))
                        }),
                    )
                } else {
                    Box::new(actix::fut::wrap_future::<_, Self>(future::ok(
                        Response::Now(custom::Event::Complete(custom::Complete {
                            oid: download.object.oid.clone(),
                            error: Some(custom::Error {
                                code: 404,
                                message: "Object not found".to_string(),
                            }),
                            path: None,
                        })),
                    )))
                }
            }
            // Upload transfer is dummy, the smudge filter adds files to IPFS already
            // TODO: just check the sha256 hash with a /api/v0/block/get
            (custom::Event::Upload(upload), custom::Operation::Upload) => {
                Box::new(actix::fut::wrap_future::<_, Self>(future::ok(
                    Response::Now(custom::Event::Complete(custom::Complete {
                        oid: upload.object.oid,
                        error: None,
                        path: None,
                    })),
                )))
            }
            (event, _) => Box::new(actix::fut::wrap_future::<_, Self>(future::err(
                CliError::UnexpectedEvent(event),
            ))),
        }
    }
}
