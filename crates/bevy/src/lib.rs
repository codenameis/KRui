//! # `krui-bevy`
//!
//! Optional Bevy 0.15 engine adapter crate sharing `wgpu 23` without polluting `krui-core`.
//!
//! Provides two complementary integration modes:
//! - **Mode A (`mode_a::BevyEditorViewportHost`)**: Host Bevy's 3D scene in a KRui editor shell via
//!   an offscreen `wgpu::TextureView` (`ExternalTextureId`) + bidirectional ECS Inspector sync.
//! - **Mode B (`mode_b::BevyInPassHudHost`)**: Inject KRui HUD overlays directly into Bevy's render
//!   pass by sharing Bevy's `wgpu::Device` & `wgpu::Queue` (`krui-engine-host` + `LoadOp::Load`).
//!
//! Enable feature `bevy-ecs` for direct `bevy_ecs::world::World` + `bevy_app::Plugin` bindings.

#![forbid(unsafe_code)]

#[cfg(feature = "bevy-ecs")]
pub mod bevy_world;
pub mod ecs_sync;
pub mod mode_a;
pub mod mode_b;

#[cfg(feature = "bevy-ecs")]
pub use bevy_world::{BevyWorldAdapter, KruiBevyHudResource, KruiBevyPlugin, KruiInspectable};
pub use ecs_sync::{
    EcsSyncBridge, EntityHandle, EntityNodeSnapshot, InspectorPatch, ReactiveEcsInspector,
    TransformSnapshot,
};
pub use mode_a::{BevyEditorConfig, BevyEditorViewportHost};
pub use mode_b::{BevyHudConfig, BevyInPassHudHost};
#[cfg(feature = "bevy-ecs")]
pub use {bevy_app, bevy_ecs};
