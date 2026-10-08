import {Dimensions, Platform} from 'react-native';
import {
  lumenAlpha,
  lumenColors,
  lumenRadius,
  lumenTypography,
  providerColor,
} from '@ristle/lumen-ui';

/**
 * Lumen — Ristle's warm, premium design direction.
 *
 * Amber is treated as a *glow*, never a flat fill. Source identity lives in our
 * chrome as a coloured dot plus a text label (never painted over album art),
 * which keeps provider attribution visible as required by the product brief.
 *
 * Fonts: headlines use Schibsted Grotesk and numbers use DM Mono (bundled as
 * static weights under assets/fonts); body/UI text uses the platform system
 * font (see `text` below). The brand families are bundled and linked
 * via react-native.config.js. Each weight is its own family whose name ==
 * PostScript name == file name, so a single string resolves on Android, iOS and
 * Windows. Weight is baked into the family, so DON'T also set fontWeight on text
 * that uses these families (it can trigger synthetic bolding on Android).
 */

export const palette = {
  // Warm, near-black browns — the base surfaces.
  base: lumenColors.base,
  baseRaise: '#241d15',
  baseGlow: '#2a2015',
  bezel: lumenColors.deepest,
  art: '#2a2219',

  // Translucent surfaces layered over the base.
  surface: lumenAlpha.surface,
  surfaceStrong: lumenAlpha.surfaceStrong,
  hairline: lumenAlpha.hairline,

  // Amber glow + ember gradient stand-ins.
  amber: lumenColors.amber,
  amberHi: lumenColors.amberHigh,
  ember: lumenColors.ember,
  amberWash: lumenAlpha.amberWash,
  amberWashStrong: lumenAlpha.amberWashStrong,
  amberLine: lumenAlpha.amberLine,

  // Cream + muted warm text.
  ink: lumenColors.textBright, // brightest headline cream
  text: lumenColors.text, // primary text
  textSoft: '#e8ddc9', // text resting on artwork tones
  muted: '#a99e8d',
  mutedCool: lumenColors.textSecondary,
  faint: lumenColors.textMuted,
  dim: lumenColors.textDim,
  onAmber: lumenColors.onAmber,

  // Controls.
  track: '#3a322a',
  danger: '#ef7a5e',
} as const;

/** Per-source identity colours used for the chrome dots. */
export const sourceColors = {
  green: '#4ad17a',
  orange: '#ff8a3d',
  grey: '#9aa0ac',
  blue: '#5ab0ff',
} as const;

/** Map a providerId to its source-identity colour. */
export function sourceColor(providerId: string): string {
  return providerColor(providerId);
}

/** Preserve the HTML baseline's 18px gutter on its 282px inner screen. */
export const gutter = Math.min(
  Math.round(Dimensions.get('window').width * (18 / 282)),
  30,
);

/** Mini-player inset: 10px on the HTML baseline's 282px inner screen. */
export const miniPlayerGutter = Math.min(
  Math.round(Dimensions.get('window').width * (10 / 282)),
  18,
);

export const radius = {
  sm: lumenRadius.small,
  md: lumenRadius.medium,
  lg: lumenRadius.large,
  xl: 18,
  xxl: 26,
  pill: 999,
} as const;

/**
 * Bundled brand font families (weight baked in — do not pair with fontWeight).
 * Schibsted Grotesk = headlines, Hanken Grotesk = UI, DM Mono = numbers only.
 */
export const fonts = {
  display: lumenTypography.display,
  heading: lumenTypography.headline,
  headingSemi: lumenTypography.headlineSemibold,
  mono: lumenTypography.numeric,
  monoMedium: lumenTypography.numericMedium,
} as const;

/**
 * Body / UI text uses the platform SYSTEM font (SF Pro on iOS, Roboto on
 * Android, Segoe UI on Windows) per the per-platform spec — only headlines use
 * Schibsted. Expressed as weight-only fragments: with no fontFamily, RN renders
 * the platform system font. (The bundled Hanken Grotesk files are kept but
 * currently unused.)
 */
export const text = {
  body: {fontWeight: '400'},
  medium: {fontWeight: '500'},
  semibold: {fontWeight: '600'},
  bold: {fontWeight: '700'},
} as const;

/** Primary play button shape: squircle on Android (M3), circle elsewhere. */
export const fabShape =
  Platform.OS === 'android'
    ? {width: 72, height: 60, borderRadius: 22}
    : {width: 66, height: 66, borderRadius: 33};
export const fabRadius = fabShape.borderRadius;
