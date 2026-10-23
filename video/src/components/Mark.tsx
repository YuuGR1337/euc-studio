import React from "react";
import { C } from "../theme";

/** The studio's icon (app/public/icon.svg): a wheel in the accent on black. */
export const Mark: React.FC<{ size: number }> = ({ size }) => (
  <svg width={size} height={size} viewBox="0 0 64 64" style={{ display: "block" }}>
    <rect width="64" height="64" rx="14" fill="#000" stroke={C.hairlineStrong} strokeWidth="1" />
    <g fill="none" stroke={C.accent} strokeWidth="5" strokeLinecap="round">
      <circle cx="32" cy="32" r="18" />
      <circle cx="32" cy="32" r="7" />
      <path d="M32 14v11" />
    </g>
  </svg>
);
