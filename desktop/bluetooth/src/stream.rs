//! RFCOMM stream implementation for Windows

use std::io::{Read, Write};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::sync::Arc;

use bytes::BytesMut;
use futures::ready;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tracing::{debug, error, trace};
use windows::Networking::Sockets::StreamSocket;
use windows::Storage::Streams::{DataReader, DataWriter, IInputStream, IOutputStream, InputStreamOptions};
use windows::Foundation::TimeSpan;

use crate::{StreamConfig, ConnectionParams};
use bpl_protocol::{ProtocolError, Result};

/// RFCOMM stream for async read/write operations
pub struct RfcommStream {
    socket: StreamSocket,
    reader: DataReader,
    writer: DataWriter,
    config: StreamConfig,
    read_buffer: BytesMut,
    write_buffer: BytesMut,
    closed: bool,
}

impl RfcommStream {
    /// Create from existing socket
    pub fn from_socket(socket: StreamSocket) -> Self {
        let reader = DataReader::CreateDataReader(socket.InputStream().unwrap()).unwrap();
        let writer = DataWriter::CreateDataWriter(socket.OutputStream().unwrap()).unwrap();

        // Configure for efficient reading
        reader.InputStreamOptions(InputStreamOptions::Partial).unwrap();

        Self {
            socket,
            reader,
            writer,
            config: StreamConfig::default(),
            read_buffer: BytesMut::with_capacity(8192),
            write_buffer: BytesMut::with_capacity(8192),
            closed: false,
        }
    }

    /// Get the underlying socket
    pub fn socket(&self) -> &StreamSocket {
        &self.socket
    }

    /// Get remote address
    pub fn remote_address(&self) -> Option<String> {
        self.socket.Information()
            .and_then(|info| info.RemoteAddress())
            .map(|addr| addr.ToString().unwrap_or_default().to_string())
    }

    /// Get remote port
    pub fn remote_port(&self) -> Option<String> {
        self.socket.Information()
            .and_then(|info| info.RemoteServiceName())
            .map(|name| name.to_string())
    }

    /// Check if stream is closed
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Set read timeout
    pub fn set_read_timeout(&mut self, timeout_ms: u32) {
        self.config.read_timeout_ms = timeout_ms;
    }

    /// Set write timeout
    pub fn set_write_timeout(&mut self, timeout_ms: u32) {
        self.config.write_timeout_ms = timeout_ms;
    }

    /// Read into buffer
    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if self.closed {
            return Err(ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Stream closed",
            )));
        }

        // Load data into reader
        let target_len = buf.len() as u32;
        let loaded = self.reader.LoadAsync(target_len)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?
            .await
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        if loaded == 0 {
            // EOF
            self.closed = true;
            return Ok(0);
        }

        // Read bytes from reader
        let mut temp_buf = vec![0u8; loaded as usize];
        self.reader.ReadBytes(&mut temp_buf)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        buf[..loaded as usize].copy_from_slice(&temp_buf);
        Ok(loaded as usize)
    }

    /// Write from buffer
    pub async fn write(&mut self, buf: &[u8]) -> Result<usize> {
        if self.closed {
            return Err(ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Stream closed",
            )));
        }

        self.writer.WriteBytes(buf)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        let stored = self.writer.StoreAsync()
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?
            .await
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        Ok(stored as usize)
    }

    /// Flush write buffer
    pub async fn flush(&mut self) -> Result<()> {
        if self.closed {
            return Err(ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Stream closed",
            )));
        }

        self.writer.FlushAsync()
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?
            .await
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        Ok(())
    }

    /// Close the stream
    pub async fn close(&mut self) -> Result<()> {
        if !self.closed {
            self.closed = true;
            // Closing the socket will close the streams
            self.socket.Close()
                .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;
        }
        Ok(())
    }

    /// Get connection statistics
    pub fn stats(&self) -> StreamStats {
        StreamStats {
            bytes_read: self.read_buffer.len() as u64,
            bytes_written: self.write_buffer.len() as u64,
            closed: self.closed,
        }
    }
}

/// Stream statistics
#[derive(Debug, Clone, Default)]
pub struct StreamStats {
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub closed: bool,
}

/// Stream configuration
#[derive(Debug, Clone)]
pub struct StreamConfig {
    pub read_timeout_ms: u32,
    pub write_timeout_ms: u32,
    pub buffer_size: usize,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            read_timeout_ms: 30000,
            write_timeout_ms: 30000,
            buffer_size: 8192,
        }
    }
}

/// Connection parameters
#[derive(Debug, Clone)]
pub struct ConnectionParams {
    pub mtu: u16,
    pub timeout_ms: u32,
    pub retry_count: u32,
    pub retry_delay_ms: u32,
}

impl Default for ConnectionParams {
    fn default() -> Self {
        Self {
            mtu: 1024,
            timeout_ms: 10000,
            retry_count: 3,
            retry_delay_ms: 1000,
        }
    }
}

/// AsyncRead implementation for RfcommStream
impl AsyncRead for RfcommStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<()>> {
        // Try to read from internal buffer first
        if !self.read_buffer.is_empty() {
            let len = std::cmp::min(buf.remaining(), self.read_buffer.len());
            buf.put_slice(&self.read_buffer[..len]);
            self.read_buffer.advance(len);
            return Poll::Ready(Ok(()));
        }

        // Load more data
        let target_len = buf.remaining().min(self.config.buffer_size) as u32;
        let load_op = self.reader.LoadAsync(target_len)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        // Poll the load operation
        let loaded = ready!(Pin::new(&mut load_op.into()).poll(cx))
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        if loaded == 0 {
            self.closed = true;
            return Poll::Ready(Ok(()));
        }

        // Read from reader into buffer
        let mut temp_buf = vec![0u8; loaded as usize];
        self.reader.ReadBytes(&mut temp_buf)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        let len = std::cmp::min(buf.remaining(), temp_buf.len());
        buf.put_slice(&temp_buf[..len]);

        // Store remaining in internal buffer
        if len < temp_buf.len() {
            self.read_buffer.extend_from_slice(&temp_buf[len..]);
        }

        Poll::Ready(Ok(()))
    }
}

/// AsyncWrite implementation for RfcommStream
impl AsyncWrite for RfcommStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize>> {
        if self.closed {
            return Poll::Ready(Err(ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Stream closed",
            ))));
        }

        // Write to writer
        self.writer.WriteBytes(buf)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        let store_op = self.writer.StoreAsync()
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        let written = ready!(Pin::new(&mut store_op.into()).poll(cx))
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        Poll::Ready(Ok(written as usize))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        if self.closed {
            return Poll::Ready(Err(ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Stream closed",
            ))));
        }

        let flush_op = self.writer.FlushAsync()
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        ready!(Pin::new(&mut flush_op.into()).poll(cx))
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        if !self.closed {
            // Flush then close
            let flush_op = self.writer.FlushAsync()
                .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

            ready!(Pin::new(&mut flush_op.into()).poll(cx))
                .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

            self.socket.Close()
                .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;
            self.closed = true;
        }
        Poll::Ready(Ok(()))
    }
}

/// Create a new stream from socket
pub fn from_socket(socket: StreamSocket) -> RfcommStream {
    RfcommStream::from_socket(socket)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_config_default() {
        let config = StreamConfig::default();
        assert_eq!(config.buffer_size, 8192);
    }
}