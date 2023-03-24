use std::{task::{Poll, Context}, pin::Pin};

use tokio::{fs::File, io::{BufReader, BufWriter, self, AsyncRead, AsyncWrite}};

pub struct NonSeekingAsyncFile {
    //file: File,
    reader: BufReader<File>,
    writer: BufWriter<File>,
}

impl NonSeekingAsyncFile {
    pub async fn open(path: &str) -> io::Result<Self> {
        //let file = File::open(path).await?;
        let mut opt = tokio::fs::OpenOptions::new();
        let file = opt.write(true).read(true).open(&path).await?;
        let reader = BufReader::new(file.try_clone().await?);
        let writer = BufWriter::new(file);
        Ok(Self { reader, writer })
        //Ok(Self { file })
    }
}

impl AsyncRead for NonSeekingAsyncFile {
    fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.reader).poll_read(cx, buf)
    }
}

impl AsyncWrite for NonSeekingAsyncFile {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.writer).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.writer).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.writer).poll_shutdown(cx)
    }
}
