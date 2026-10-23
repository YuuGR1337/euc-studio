/*! EUC Thrills — (c) 2026 VibezZzCoder — MIT — https://github.com/VibezZzCoder/EUC-thrills */
// EUC Studio: the values the Cool Rider look and its rig read, copied unchanged from the game's
// src/data/tuning.ts (line numbers of that file in the comments). The game's other tuning is not
// part of this copy.

/** Colours, sRGB hex (BLOCKOUT_COLOURS, tuning.ts:4058-4076). */
export const BLOCKOUT_COLOURS = {
  /** Cool Rider's detailed garment palette. */
  coolFabric: 0x606570,
  coolHelmet: 0x424b59,
  coolVisor: 0x8daab5,
  coolLash: 0x302a23,
  coolGear: 0x4c5059,
  coolSkin: 0xb98a6b,
  coolSkinShadow: 0x87614f,
  coolSole: 0xa6a69b,
  coolStitch: 0x727982,
  coolInk: 0x20252c,
  coolPrint: 0xbac2cc,
  /** The identity cue: the reflective blue panels. */
  riderPanel: 0x2f7fe8,
} as const;

/** WHEEL.pedalHeight (tuning.ts:77): pedal tread above the ground, metres. */
export const WHEEL = {
  pedalHeight: 0.16,
} as const;

/** RIDER (tuning.ts:100-105), metres. */
export const RIDER = {
  /** Standing height, helmet included. */
  height: 1.75,
  /** Ground to hip joint when riding, knees softly bent. */
  hipHeight: 0.92,
} as const;

/** The rider's skeleton and arm carriage (RIDER_BLOCKOUT, tuning.ts:119-442), metres and radians. */
export const RIDER_BLOCKOUT = {
  /** Ankle joint height above the pedal tread (:121). */
  ankleAbovePedal: 0.06,
  shinLength: 0.38, // :133
  thighLength: 0.4, // :134
  stanceHalfWidth: 0.185, // :136
  torsoLength: 0.5, // :143
  torsoWidth: 0.34, // :145
  torsoDepth: 0.27, // :146
  torsoRestPitch: 0.1, // :148
  headStabilizationFactor: 0.65, // :276
  headStabilizationMax: 0.55, // :277
  armAsymmetrySplay: 0.03, // :288
  armAsymmetryForward: 0.035, // :289
  shoulderHalfWidth: 0.175, // :320
  upperArmLength: 0.28, // :321
  forearmLength: 0.26, // :322
  armSplay: 0.115, // :407
  armHangFraction: 0.93, // :408
  handForward: 0.12, // :410
  armEaseElbow: 2.42, // :433
  armEaseElbowRightBend: 0.15, // :434
  armEaseIn: 0.02, // :435
  armEaseForward: 0.02, // :436
  armEaseRiseMax: 0.07, // :437
  armEaseElbowOut: 0.6, // :438
  armEaseElbowOutRight: 0.45, // :439
  armEaseHandRoll: 0.9, // :440
  armEaseFullBelow: 4.0, // :441
  armEaseGoneAbove: 16.0, // :442
} as const;
