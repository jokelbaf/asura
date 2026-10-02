use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::sync::Arc;

use control::{Reply, Request};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use super::{Origin, Service};

pub async fn serve(service: Arc<Service>) -> io::Result<()> {
    let path = control::socket();

    if let Some(directory) = path.parent() {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)?;
    }

    let _ = fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;

    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(handle(service.clone(), stream));
    }
}

async fn handle(service: Arc<Service>, stream: UnixStream) -> io::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut line = String::new();
    BufReader::new(reader).read_line(&mut line).await?;

    let reply = match serde_json::from_str(&line) {
        Ok(request) => dispatch(service, request).await,
        Err(error) => Reply::Failed(error.to_string()),
    };

    let mut message = serde_json::to_vec(&reply)?;
    message.push(b'\n');
    writer.write_all(&message).await
}

async fn dispatch(service: Arc<Service>, request: Request) -> Reply {
    let Request::Register {
        name,
        process,
        command,
    } = request;

    let outcome = tokio::task::spawn_blocking(move || {
        service.enrol(&name, process, &command.join(" "), Origin::Launched)
    })
    .await;

    match outcome {
        Ok(Ok(())) => Reply::Registered { capturing: true },
        Ok(Err(error)) => Reply::Failed(format!("{error:#}")),
        Err(error) => Reply::Failed(error.to_string()),
    }
}
