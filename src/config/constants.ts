// Core configuration constants for Geometry Scroller Game

/** Physics related constants */
export const PHYSICS = {
  /** Gravity applied to player (pixels/s²) */
  GRAVITY: 980,
  /** Friction coefficient applied each frame */
  FRICTION: 0.92,
  /** Terminal velocity (pixels/s) */
  TERMINAL_VELOCITY: 1500,
} as const;

/** Speed curve constants for difficulty scaling */
export const SPEED_CURVE = {
  /** Base speed (pixels/s) */
  BASE: 200,
  /** Multiplier per level */
  LEVEL_MULTIPLIER: 1.15,
  /** Maximum speed */
  MAX: 2000,
} as const;

/** Spawn window timing constants */
export const SPAWN_WINDOW = {
  /** Minimum time between spawns (ms) */
  MIN_INTERVAL: 300,
  /** Maximum time between spawns (ms) */
  MAX_INTERVAL: 1500,
  /** Random variance added */
  VARIANCE: 200,
} as const;
