// Core type definitions for Geometry Scroller Game

/**
 * Represents the mutable state of the player during a run.
 */
export interface PlayerState {
  /** Current x‑position in world coordinates */
  x: number;
  /** Current y‑position in world coordinates */
  y: number;
  /** Current health points (0‑100) */
  health: number;
  /** Current score */
  score: number;
  /** Current velocity in pixels per second */
  velocity: number;
}

/**
 * Schema for a single obstacle placed in a level.
 */
export interface ObstacleSchema {
  /** Identifier for the obstacle type */
  type: string;
  /** Horizontal position (world coordinates) */
  x: number;
  /** Vertical position (world coordinates) */
  y: number;
  /** Optional width, defaults to sprite width */
  width?: number;
  /** Optional height, defaults to sprite height */
  height?: number;
}

/**
 * Complete level description used by the engine to generate a run.
 */
export interface LevelSchema {
  /** Human readable name */
  name: string;
  /** Width of the level world */
  width: number;
  /** Height of the level world */
  height: number;
  /** Collection of obstacles that appear in this level */
  obstacles: ObstacleSchema[];
}

/**
 * Snapshot of a full run used for analytics or replay.
 */
export interface RunSnapshot {
  /** Timestamp when the run started (ms since epoch) */
  startTime: number;
  /** Timestamp when the run ended */
  endTime: number;
  /** Final player state */
  finalState: PlayerState;
  /** All levels traversed */
  levels: LevelSchema[];
}

/**
 * Event emitted when the player unlocks an achievement.
 */
export interface AchievementEvent {
  /** Unique key of the achievement */
  id: string;
  /** Human readable description */
  description: string;
  /** Timestamp of unlocking */
  unlockedAt: number;
}
