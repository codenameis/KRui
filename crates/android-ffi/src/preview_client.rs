//! Feature-gated TCP live preview client bridge (`krui serve --android`) for Android hosts.

use krui_core::ViewTree;

/// Wraps an optional `krui-preview` TCP client without leaking compiler dependencies into release builds.
#[derive(Default)]
pub struct AndroidPreviewBridge {
    #[cfg(feature = "preview")]
    client: Option<krui_preview::PreviewTcpClient>,
    #[cfg(feature = "preview")]
    compile_error: Option<krui_compiler::CompileError>,
    #[cfg(feature = "preview")]
    reload_count: u64,
}

impl AndroidPreviewBridge {
    /// Connects to a `krui serve --android` TCP server at `addr` when built with `--features preview`.
    pub fn connect(&mut self, addr: &str) -> bool {
        #[cfg(feature = "preview")]
        {
            if !cfg!(debug_assertions) {
                let _ = addr;
                return false;
            }
            if let Some(client) = krui_preview::PreviewTcpClient::connect(addr.trim()) {
                self.client = Some(client);
                return true;
            }
            false
        }
        #[cfg(not(feature = "preview"))]
        {
            let _ = addr;
            false
        }
    }

    /// Polls incoming `PreviewCommand`s and hot-reloads `tree` if updates arrived.
    pub fn poll(&mut self, tree: &mut ViewTree) -> bool {
        #[cfg(feature = "preview")]
        {
            if !cfg!(debug_assertions) {
                let _ = tree;
                return false;
            }
            if let Some(client) = &mut self.client {
                return client.poll(tree, &mut self.compile_error, &mut self.reload_count);
            }
            false
        }
        #[cfg(not(feature = "preview"))]
        {
            let _ = tree;
            false
        }
    }

    /// Returns the number of preview reloads applied during this session (`0` in release builds).
    pub fn reload_count(&self) -> u64 {
        #[cfg(feature = "preview")]
        {
            self.reload_count
        }
        #[cfg(not(feature = "preview"))]
        {
            0
        }
    }

    /// Returns the last structured compile error string, if any (`None` in release builds).
    pub fn last_error_message(&self) -> Option<String> {
        #[cfg(feature = "preview")]
        {
            self.compile_error.as_ref().map(|err| err.to_string())
        }
        #[cfg(not(feature = "preview"))]
        {
            None
        }
    }
}
