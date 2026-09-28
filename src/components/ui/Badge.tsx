import { HTMLAttributes, forwardRef } from 'react';
import { clsx } from 'clsx';

export interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  variant?:
    | 'default'
    | 'secondary'
    | 'destructive'
    | 'outline'
    | 'success'
    | 'warning'
    | 'danger'
    | 'neutral'
    | 'accent';
}

/**
 * Status badges.
 *
 * `success` / `warning` / `danger` are the only variants allowed to carry green,
 * amber and red, because those three colours mean one thing each in this app:
 * ready, needs you, failed. Spending them on anything decorative would make the
 * status list unreadable, so a neutral or accent badge is used wherever a
 * second status colour would be noise.
 *
 * `rounded`, not `rounded-full`. A 20px pill reads as a web tag; a macOS badge is
 * a small rounded rectangle.
 */
export const Badge = forwardRef<HTMLSpanElement, BadgeProps>(
  ({ className, variant = 'default', ...props }, ref) => {
    const variants: Record<NonNullable<BadgeProps['variant']>, string> = {
      default: 'border-transparent bg-primary text-primary-foreground',
      secondary: 'border-border bg-surface-hover text-fg-muted',
      destructive: 'border-danger-border bg-danger-bg text-danger-fg',
      outline: 'border-border text-foreground',
      success: 'border-success-border bg-success-bg text-success-fg',
      warning: 'border-warning-border bg-warning-bg text-warning-fg',
      danger: 'border-danger-border bg-danger-bg text-danger-fg',
      neutral: 'border-neutral-border bg-neutral-bg text-neutral-fg',
      accent: 'border-primary-soft-border bg-primary-soft text-primary',
    };

    return (
      <span
        ref={ref}
        className={clsx(
          'inline-flex h-5 shrink-0 items-center gap-1 whitespace-nowrap rounded border px-1.5 text-[11px] font-medium leading-none',
          variants[variant],
          className
        )}
        {...props}
      />
    );
  }
);
Badge.displayName = 'Badge';
