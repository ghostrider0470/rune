export const designSystem = {
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
