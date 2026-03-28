export const designSystem = {
  color: {
    palette: {
      light: {
        background: 'oklch(0.985 0.012 248)',
        foreground: 'oklch(0.28 0.04 254)',
        card: 'oklch(0.995 0.01 248)',
        primary: 'oklch(0.65 0.25 35)',
        secondary: 'oklch(0.955 0.02 250)',
        accent: 'oklch(0.7 0.2 285)',
        muted: 'oklch(0.94 0.018 248)',
        border: 'oklch(0.9 0.02 250)',
      },
      dark: {
        background: 'oklch(0.17 0.02 255)',
        foreground: 'oklch(0.95 0.015 255)',
        card: 'oklch(0.22 0.02 255)',
        primary: 'oklch(0.75 0.22 35)',
        secondary: 'oklch(0.32 0.03 258)',
        accent: 'oklch(0.75 0.18 285)',
        muted: 'oklch(0.27 0.02 255)',
        border: 'oklch(1 0 0 / 14%)',
      },
      semantic: {
        success: 'oklch(0.72 0.18 152)',
        warning: 'oklch(0.82 0.17 84)',
        error: 'oklch(0.64 0.24 25)',
        info: 'oklch(0.7 0.15 245)',
      },
    },
    gradients: {
      brand: 'from-primary via-accent to-primary',
      sunrise: 'from-primary/20 via-accent/15 to-background',
      mesh: 'from-primary/12 via-background to-accent/12',
    },
  },
  typography: {
    display: {
      hero: 'text-4xl font-bold tracking-tight sm:text-5xl lg:text-6xl xl:text-7xl',
      heroCompact: 'text-3xl font-bold tracking-tight sm:text-4xl lg:text-5xl',
      pageTitle: 'text-3xl font-bold tracking-tight sm:text-4xl lg:text-5xl',
      sectionTitle: 'text-2xl font-bold tracking-tight sm:text-3xl lg:text-4xl',
      eyebrow: 'text-xs font-semibold uppercase tracking-[0.14em] text-muted-foreground',
    },
    heading: {
      h1: 'text-3xl font-bold tracking-tight sm:text-4xl lg:text-5xl',
      h2: 'text-2xl font-bold tracking-tight sm:text-3xl lg:text-4xl',
      h3: 'text-xl font-semibold sm:text-2xl',
      h4: 'text-xl font-semibold',
      h5: 'text-lg font-medium',
      h6: 'text-base font-medium',
    },
    body: {
      large: 'text-lg',
      base: 'text-base',
      small: 'text-sm',
      xs: 'text-xs',
      lead: 'text-lg leading-relaxed sm:text-xl',
    },
    muted: 'text-muted-foreground',
  },
  spacing: {
    scale: {
      1: '0.25rem',
      2: '0.5rem',
      3: '0.75rem',
      4: '1rem',
      5: '1.25rem',
      6: '1.5rem',
      8: '2rem',
      10: '2.5rem',
      12: '3rem',
      16: '4rem',
      20: '5rem',
      24: '6rem',
    },
    page: {
      container: 'container mx-auto px-4 sm:px-6 lg:px-8',
      section: 'py-8 md:py-12',
      sectionCompact: 'py-6 md:py-8',
      sectionLarge: 'py-12 md:py-16',
      header: 'mb-6 md:mb-8',
    },
    section: {
      sm: 'py-4 md:py-6',
      md: 'py-6 md:py-8',
      lg: 'py-8 md:py-12',
      xl: 'py-12 md:py-16',
    },
    component: {
      card: 'p-6',
      cardCompact: 'p-4',
      button: {
        sm: 'px-3 py-1.5',
        md: 'px-4 py-2',
        lg: 'px-6 py-3',
      },
    },
    gap: {
      xs: 'gap-2',
      sm: 'gap-3',
      md: 'gap-4',
      lg: 'gap-6',
      xl: 'gap-8',
    },
  },
  grid: {
    responsive: {
      two: 'grid grid-cols-1 md:grid-cols-2',
      three: 'grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3',
      four: 'grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4',
    },
  },
  surfaces: {
    section: {
      default: 'bg-transparent',
      subtle: 'bg-muted/20',
      muted: 'bg-muted/40',
      accent: 'bg-primary/5',
      elevated: 'bg-card',
      gradient: 'bg-gradient-to-b from-background to-muted/20',
    },
    cta: {
      default: 'bg-gradient-to-r from-primary/10 via-accent/10 to-primary/10',
      warm: 'bg-gradient-to-r from-primary/15 via-primary/10 to-accent/15',
      cool: 'bg-gradient-to-r from-accent/12 via-background to-primary/12',
      mesh: 'bg-gradient-to-br from-primary/10 via-background to-accent/10',
    },
    card: {
      default: 'border bg-card text-card-foreground',
      subtle: 'border border-muted/60 bg-muted/20 text-card-foreground',
      accent: 'border border-primary/30 bg-primary/5 text-card-foreground',
      gradient: 'border border-primary/20 bg-gradient-to-br from-card to-primary/5 text-card-foreground',
      glass: 'border border-border/60 bg-background/70 text-card-foreground backdrop-blur',
    },
  },
  components: {
    button: {
      base: 'inline-flex items-center justify-center gap-2 rounded-md text-sm font-medium transition-all',
      emphasis: 'bg-primary text-primary-foreground hover:bg-primary/90',
      subtle: 'border bg-background hover:bg-accent hover:text-accent-foreground',
      ghost: 'hover:bg-accent/60 hover:text-accent-foreground',
    },
    card: {
      base: 'rounded-xl border bg-card text-card-foreground shadow-sm',
      interactive: 'transition-all hover:border-primary/30 hover:shadow-lg',
      hero: 'rounded-2xl border border-primary/20 bg-gradient-to-br from-card to-primary/5 shadow-lg',
    },
    badge: {
      neutral: 'border bg-background text-foreground',
      emphasis: 'bg-primary text-primary-foreground',
      subtle: 'bg-muted text-muted-foreground',
    },
    field: {
      input: 'h-10 rounded-md border border-input bg-background px-3 text-sm',
      textarea: 'min-h-24 rounded-md border border-input bg-background px-3 py-2 text-sm',
      label: 'text-sm font-medium text-foreground',
    },
    modal: {
      content: 'rounded-2xl border bg-card shadow-2xl',
    },
    table: {
      shell: 'rounded-xl border bg-card',
      rowInteractive: 'transition-colors hover:bg-muted/40',
    },
  },
  effects: {
    card: {
      base: 'border bg-card text-card-foreground shadow-sm',
      hover: 'transition-shadow hover:shadow-lg',
      interactive: 'cursor-pointer transition-all hover:shadow-lg hover:scale-[1.02]',
      elevated: 'shadow-md hover:shadow-xl',
      accent: 'border-primary/30 bg-primary/5',
      muted: 'border-muted/60 bg-muted/20',
    },
    gradient: {
      subtle: 'bg-gradient-to-br from-background to-muted/20',
      primary: 'bg-gradient-to-r from-primary/10 to-primary/5',
      muted: 'bg-gradient-to-br from-muted/50 to-background',
      cta: 'bg-gradient-to-r from-primary/10 via-accent/10 to-primary/10',
      ctaStrong: 'bg-gradient-to-r from-primary/20 via-accent/20 to-primary/20',
    },
    focusRing: 'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background',
  },
  icons: {
    size: {
      xs: 'h-3 w-3',
      sm: 'h-4 w-4',
      md: 'h-5 w-5',
      lg: 'h-6 w-6',
      xl: 'h-8 w-8',
      hero: 'h-10 w-10',
    },
    wrapper: {
      sm: 'h-10 w-10 rounded-xl p-2',
      md: 'h-12 w-12 rounded-xl p-2.5',
      lg: 'h-16 w-16 rounded-2xl p-3',
      hero: 'h-20 w-20 rounded-2xl p-4',
    },
  },
  animation: {
    loading: 'animate-spin',
    fadeIn: 'animate-in fade-in duration-500',
    slideUp: 'animate-in slide-in-from-bottom-4 duration-500',
    motion: {
      duration: {
        fast: 0.3,
        base: 0.4,
        slow: 0.5,
      },
      ease: {
        out: [0.16, 1, 0.3, 1] as const,
      },
      distance: {
        slideUp: 16,
        page: 10,
      },
      stagger: {
        cards: 0.08,
      },
    },
  },
} as const

export type DesignSystem = typeof designSystem
