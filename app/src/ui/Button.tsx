import { type ButtonHTMLAttributes, type ReactNode, forwardRef } from "react";
import { cx } from "../lib/cx.ts";
import { Tooltip } from "./Tooltip.tsx";

export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";
export type ButtonSize = "sm" | "md" | "lg";

const variants: Record<ButtonVariant, string> = {
  primary:
    "bg-accent text-on-accent live:hover:bg-accent-hover force-hover:bg-accent-hover font-semibold",
  secondary:
    "bg-raised text-fg border border-hairline live:hover:bg-track force-hover:bg-track font-medium",
  ghost:
    "bg-transparent text-fg-2 live:hover:bg-raised live:hover:text-fg force-hover:bg-raised force-hover:text-fg font-medium",
  danger:
    "bg-danger-tint text-danger-text live:hover:bg-danger-tint-hover force-hover:bg-danger-tint-hover font-semibold",
};

const sizes: Record<ButtonSize, string> = {
  sm: "h-(--control-sm) px-8 gap-4 text-12 rounded-8 [&_svg]:size-[14px]",
  md: "h-(--control-md) px-16 gap-8 text-13 rounded-8 [&_svg]:size-[16px]",
  lg: "h-(--control-lg) px-24 gap-8 text-15 rounded-12 [&_svg]:size-[18px]",
};

const iconSizes: Record<ButtonSize, string> = {
  sm: "size-(--control-sm) rounded-8 [&_svg]:size-[14px]",
  md: "size-(--control-md) rounded-8 [&_svg]:size-[18px]",
  lg: "size-(--control-lg) rounded-12 [&_svg]:size-[20px]",
};

const base =
  "pressable relative inline-flex shrink-0 select-none items-center justify-center whitespace-nowrap " +
  "aria-disabled:opacity-40 disabled:opacity-40";

export interface ButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "disabled"> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** Leading icon. */
  icon?: ReactNode;
  /** Disables the button and explains why in its tooltip. A disabled control always says why. */
  disabledReason?: string | undefined;
  /** Gallery only: render a state statically. */
  force?: "hover" | "press" | "focus";
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  {
    variant = "secondary",
    size = "md",
    icon,
    disabledReason,
    force,
    className,
    children,
    onClick,
    ...rest
  },
  ref,
) {
  const disabled = !!disabledReason;
  const button = (
    <button
      ref={ref}
      type="button"
      aria-disabled={disabled || undefined}
      data-force={force}
      className={cx(base, variants[variant], sizes[size], className)}
      onClick={disabled ? (e) => e.preventDefault() : onClick}
      {...rest}
    >
      {icon}
      {children}
    </button>
  );
  return disabled ? <Tooltip content={disabledReason}>{button}</Tooltip> : button;
});

export interface IconButtonProps extends Omit<ButtonProps, "icon" | "children"> {
  /** Accessible name, also shown in the tooltip. */
  label: string;
  shortcut?: string | undefined;
  icon: ReactNode;
  /** Toggled-on state (for example a dock that is open). */
  active?: boolean;
  tooltipSide?: "top" | "right" | "bottom" | "left";
}

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  {
    label,
    shortcut,
    icon,
    variant = "ghost",
    size = "md",
    active,
    disabledReason,
    tooltipSide,
    force,
    className,
    onClick,
    ...rest
  },
  ref,
) {
  const disabled = !!disabledReason;
  return (
    <Tooltip
      content={disabledReason ?? label}
      shortcut={disabled ? undefined : shortcut}
      {...(tooltipSide ? { side: tooltipSide } : {})}
    >
      <button
        ref={ref}
        type="button"
        aria-label={label}
        aria-disabled={disabled || undefined}
        aria-pressed={active}
        data-force={force}
        className={cx(
          base,
          variants[variant],
          iconSizes[size],
          active && variant === "ghost" && "bg-raised text-fg",
          className,
        )}
        onClick={disabled ? (e) => e.preventDefault() : onClick}
        {...rest}
      >
        {icon}
      </button>
    </Tooltip>
  );
});
