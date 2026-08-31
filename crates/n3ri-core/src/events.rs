//! Core event types for cross-plugin communication.
//!
//! All state transitions and system commands flow through these Events.
//! Each subsystem plugin registers its own Event readers.

use bevy::prelude::*;

// ── Boot / Loading ──

/// Boot animation completed — transition to Loading state.
#[derive(Message, Clone)]
pub struct BootCompleteEvent;

/// Asset loading completed — transition to Desktop state.
#[derive(Message, Clone)]
pub struct LoadCompleteEvent;

/// Loading progress update (0.0 to 1.0).
#[derive(Message, Clone)]
pub struct LoadProgressEvent {
    pub progress: f32,
    pub phase: String,
}

// ── Application lifecycle ──

/// Launch an application.
#[derive(Message, Clone)]
pub struct AppLaunchEvent {
    pub app_id: String,
    pub window_title: Option<String>,
}

/// Application requested to close.
#[derive(Message, Clone)]
pub struct AppCloseEvent {
    pub app_id: String,
}

/// Application gained/lost focus.
#[derive(Message, Clone)]
pub struct AppFocusEvent {
    pub app_id: String,
    pub focused: bool,
}

// ── System UI ──

/// Show/hide system menu.
#[derive(Message, Clone)]
pub struct SystemMenuEvent {
    pub open: bool,
}

/// Show a notification.
#[derive(Message, Clone)]
pub struct NotificationEvent {
    pub title: String,
    pub message: String,
    pub icon: Option<String>,
    pub duration: Option<f32>,
}

/// Request system shutdown.
#[derive(Message, Clone)]
pub struct ShutdownEvent {
    pub reason: Option<String>,
}

// ── Window management ──

/// Window created.
#[derive(Message, Clone)]
pub struct WindowCreatedEvent {
    pub window_id: String,
    pub title: String,
    pub app_id: String,
}

/// Window moved.
#[derive(Message, Clone)]
pub struct WindowMovedEvent {
    pub window_id: String,
    pub position: Vec2,
}

/// Window resized.
#[derive(Message, Clone)]
pub struct WindowResizedEvent {
    pub window_id: String,
    pub size: Vec2,
}

/// Window focused.
#[derive(Message, Clone)]
pub struct WindowFocusedEvent {
    pub window_id: String,
    pub focused: bool,
}
