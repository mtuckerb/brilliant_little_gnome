import { expect, test } from 'vitest';
import { PHYSICS, SPEED_CURVE, SPAWN_WINDOW } from '@/config/constants';
import { PlayerState, ObstacleSchema, LevelSchema, RunSnapshot, AchievementEvent } from '@/types/game';

// Verify that constants have expected shape
test('physics constants are numbers', () => {
  expect(typeof PHYSICS.GRAVITY).toBe('number');
  expect(typeof PHYSICS.FRICTION).toBe('number');
  expect(typeof PHYSICS.TERMINAL_VELOCITY).toBe('number');
});

test('speed curve constants are numbers', () => {
  expect(typeof SPEED_CURVE.BASE).toBe('number');
  expect(typeof SPEED_CURVE.LEVEL_MULTIPLIER).toBe('number');
  expect(typeof SPEED_CURVE.MAX).toBe('number');
});

test('spawn window constants are numbers', () => {
  expect(typeof SPAWN_WINDOW.MIN_INTERVAL).toBe('number');
  expect(typeof SPAWN_WINDOW.MAX_INTERVAL).toBe('number');
  expect(typeof SPAWN_WINDOW.VARIANCE).toBe('number');
});

// Simple type usage checks (runtime values)
test('player state conforms to interface', () => {
  const state: PlayerState = { x: 0, y: 0, health: 100, score: 0, velocity: 0 };
  expect(state.health).toBeGreaterThanOrEqual(0);
});

test('obstacle schema optional fields', () => {
  const obs: ObstacleSchema = { type: 'spike', x: 100, y: 200 };
  expect(obs.width).toBeUndefined();
});

// Negative compile test: assigning wrong type should error
// @ts-expect-error
const invalid: PlayerState = { x: 'left', y: 0, health: 100, score: 0, velocity: 0 };
