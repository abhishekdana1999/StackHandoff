import { forwardRef, ButtonHTMLAttributes } from 'react';
import { clsx } from 'clsx';

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: 'primary' | 'secondary' | 'outline' | 'ghost' | 'danger';
  size?: 'sm' | 'md' | 'lg' | 'icon';
  loading?: boolean;
}

/**
 * 28px tall at `md`, not 40px.
 *
 * The old sizes were 36/40/44px with `px-4`/`px-8`, which is a web form. A dense
 * desktop tool needs a row height that lets a list of steps and a footer of
 * actions both fit without scrolling, and the whole window is 780px tall.
 */
export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = 'primary', size = 'md', loading, disabled, children, ...props }, ref) => {
    const baseStyles =
      'inline-flex shrink-0 items-center justify-center gap-1.5 whitespace-nowrap rounded text-[13px] font-medium transition-colors ' +
      'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-background ' +
      'disabled:pointer-events-none disabled:opacity-45';

    const variants = {
      primary: 'bg-primary text-primary-foreground hover:bg-primary-hover',
      // A bordered surface rather than a filled one: on a dense screen a row of
      // grey blocks reads louder than the primary action it sits beside.
      secondary: 'border border-border bg-surface text-foreground hover:bg-surface-hover',
      outline: 'border border-hairline bg-transparent text-foreground hover:bg-surface-hover',
      ghost: 'text-fg-muted hover:bg-surface-hover hover:text-foreground',
      danger: 'bg-danger-fg text-white hover:opacity-90',
    };

    const sizes = {
      sm: 'h-6 px-2 text-[11px]',
      md: 'h-7 px-2.5',
      lg: 'h-8 px-3.5',
      icon: 'h-7 w-7 p-0',
    };

    return (
      <button
        ref={ref}
        className={clsx(baseStyles, variants[variant], sizes[size], className)}
        disabled={disabled || loading}
        {...props}
      >
        {loading && (
          <svg
            className="h-3.5 w-3.5 shrink-0 animate-spin"
            xmlns="http://www.w3.org/2000/svg"
            fill="none"
            viewBox="0 0 24 24"
            aria-hidden="true"
          >
            <circle
              className="opacity-25"
              cx="12"
              cy="12"
              r="10"
              stroke="currentColor"
              strokeWidth="4"
            />
            <path
              className="opacity-75"
              fill="currentColor"
              d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
            />
          </svg>
        )}
        {children}
      </button>
    );
  }
);
Button.displayName = 'Button';
