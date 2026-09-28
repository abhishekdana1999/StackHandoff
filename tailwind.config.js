/** @type {import('tailwindcss').Config} */
export default {
  // Class-based, not `media`: the window has an explicit Appearance control in
  // Settings and a quick toggle in the sidebar, and either can override the OS.
  // The default `media` strategy would make the toggle a no-op.
  darkMode: 'class',
  content: [
    './index.html',
    './src/**/*.{js,ts,jsx,tsx}',
  ],
  theme: {
    extend: {
      colors: {
        // The surface ladder. `canvas` is the window background, `sunken` sits
        // below it (sidebar, wells), `DEFAULT` is a raised card.
        surface: {
          DEFAULT: 'hsl(var(--surface) / <alpha-value>)',
          sunken: 'hsl(var(--surface-sunken) / <alpha-value>)',
          canvas: 'hsl(var(--surface-canvas) / <alpha-value>)',
          overlay: 'hsl(var(--surface-overlay) / <alpha-value>)',
          hover: 'hsl(var(--surface-hover) / <alpha-value>)',
          active: 'hsl(var(--surface-active) / <alpha-value>)',
        },
        // Four text tiers. Prefer these over `text-black/60`-style overrides so
        // contrast stays correct when the mode changes.
        // Four text tiers, grouped so the utilities read as `text-fg-muted`
        // rather than `text-text-muted`. Naming a colour `text-muted` collides
        // with the `text-` utility prefix and produces the doubled form.
        fg: {
          muted: 'hsl(var(--fg-muted) / <alpha-value>)',
          subtle: 'hsl(var(--fg-subtle) / <alpha-value>)',
          faint: 'hsl(var(--fg-faint) / <alpha-value>)',
        },
        // The stronger of the two rules. "Hairline" because that is what it is:
        // a 1px divider, not a "strong border".
        hairline: 'hsl(var(--hairline) / <alpha-value>)',

        primary: {
          DEFAULT: 'hsl(var(--primary) / <alpha-value>)',
          hover: 'hsl(var(--primary-hover) / <alpha-value>)',
          foreground: 'hsl(var(--primary-foreground) / <alpha-value>)',
          soft: 'hsl(var(--primary-soft) / <alpha-value>)',
          'soft-border': 'hsl(var(--primary-soft-border) / <alpha-value>)',
        },

        // Status families. These are the only place green, amber and red are
        // allowed to appear: they mean ready / needs you / failed, and spending
        // them on decoration would make that meaning ambiguous.
        success: {
          DEFAULT: 'hsl(var(--success-fg) / <alpha-value>)',
          fg: 'hsl(var(--success-fg) / <alpha-value>)',
          bg: 'hsl(var(--success-bg) / <alpha-value>)',
          border: 'hsl(var(--success-border) / <alpha-value>)',
        },
        warning: {
          DEFAULT: 'hsl(var(--warning-fg) / <alpha-value>)',
          fg: 'hsl(var(--warning-fg) / <alpha-value>)',
          bg: 'hsl(var(--warning-bg) / <alpha-value>)',
          border: 'hsl(var(--warning-border) / <alpha-value>)',
        },
        danger: {
          DEFAULT: 'hsl(var(--danger-fg) / <alpha-value>)',
          fg: 'hsl(var(--danger-fg) / <alpha-value>)',
          bg: 'hsl(var(--danger-bg) / <alpha-value>)',
          border: 'hsl(var(--danger-border) / <alpha-value>)',
        },
        // "We could not determine this" — deliberately not a status.
        neutral: {
          DEFAULT: 'hsl(var(--neutral-fg) / <alpha-value>)',
          fg: 'hsl(var(--neutral-fg) / <alpha-value>)',
          bg: 'hsl(var(--neutral-bg) / <alpha-value>)',
          border: 'hsl(var(--neutral-border) / <alpha-value>)',
        },

        // --- aliases kept for the existing screens ------------------
        background: 'hsl(var(--background) / <alpha-value>)',
        foreground: 'hsl(var(--foreground) / <alpha-value>)',
        card: {
          DEFAULT: 'hsl(var(--card) / <alpha-value>)',
          foreground: 'hsl(var(--card-foreground) / <alpha-value>)',
        },
        popover: {
          DEFAULT: 'hsl(var(--popover) / <alpha-value>)',
          foreground: 'hsl(var(--popover-foreground) / <alpha-value>)',
        },
        secondary: {
          DEFAULT: 'hsl(var(--secondary) / <alpha-value>)',
          foreground: 'hsl(var(--secondary-foreground) / <alpha-value>)',
        },
        muted: {
          DEFAULT: 'hsl(var(--muted) / <alpha-value>)',
          foreground: 'hsl(var(--muted-foreground) / <alpha-value>)',
        },
        accent: {
          DEFAULT: 'hsl(var(--accent) / <alpha-value>)',
          foreground: 'hsl(var(--accent-foreground) / <alpha-value>)',
        },
        destructive: {
          DEFAULT: 'hsl(var(--destructive) / <alpha-value>)',
          foreground: 'hsl(var(--destructive-foreground) / <alpha-value>)',
        },
        border: 'hsl(var(--border) / <alpha-value>)',
        input: 'hsl(var(--input) / <alpha-value>)',
        ring: 'hsl(var(--ring) / <alpha-value>)',
      },
      fontFamily: {
        // The design system specified Geist + JetBrains Mono. Shipping a webfont
        // to honour that would mean a network fetch on launch, which a local-only
        // app should not do. On macOS the system stack *is* SF Pro, which is the
        // platform-native choice and closer to the brief's intent than a
        // downloaded face would be. The mono stack leads with SF Mono for the
        // same reason, falling back to JetBrains Mono elsewhere.
        sans: [
          '-apple-system',
          'BlinkMacSystemFont',
          'SF Pro Text',
          'Segoe UI Variable Text',
          'Segoe UI',
          'system-ui',
          'sans-serif',
        ],
        mono: [
          'SF Mono',
          'ui-monospace',
          'JetBrains Mono',
          'Fira Code',
          'Menlo',
          'Consolas',
          'monospace',
        ],
      },
      fontSize: {
        // A compact scale, per the brief. These line up with the CSS component
        // classes in index.css.
        '2xs': ['10px', { lineHeight: '14px' }],
        xs: ['11px', { lineHeight: '16px' }],
        sm: ['12px', { lineHeight: '17px' }],
        base: ['13px', { lineHeight: '18px' }],
        md: ['14px', { lineHeight: '20px' }],
        lg: ['16px', { lineHeight: '24px', letterSpacing: '-0.01em' }],
        xl: ['20px', { lineHeight: '28px', letterSpacing: '-0.015em' }],
      },
      borderRadius: {
        // 4px base, 6px for cards. Never `full` for anything structural.
        none: '0px',
        sm: '3px',
        DEFAULT: '4px',
        md: '6px',
        lg: '8px',
        xl: '10px',
      },
      animation: {
        'fade-in': 'fadeIn 0.2s ease-out',
        'slide-up': 'slideUp 0.3s ease-out',
        'pulse-slow': 'pulse 3s cubic-bezier(0.4, 0, 0.6, 1) infinite',
      },
      keyframes: {
        fadeIn: {
          '0%': { opacity: '0' },
          '100%': { opacity: '1' },
        },
        slideUp: {
          '0%': { transform: 'translateY(10px)', opacity: '0' },
          '100%': { transform: 'translateY(0)', opacity: '1' },
        },
      },
    },
  },
  plugins: [],
};
