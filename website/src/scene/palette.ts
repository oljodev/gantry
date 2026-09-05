export type ThemeMode = 'light' | 'dark';

/** The scene's colours, mirroring the CSS tokens in styles/tokens.css. Hex values are sRGB; three converts them. */
export interface Palette {
  bg: number; fogNear: number; fogFar: number; steel: number; steelDark: number; ground: number; module: number; line: number;
  carriage: number; accent: number; particle: number;
  hemiSky: number; hemiGround: number; envIntensity: number; exposure: number; additive: boolean; bloom: number;
}

export const palettes: Record<ThemeMode, Palette> = {
  dark: {
    bg: 0x0c1116, fogNear: 30, fogFar: 150, steel: 0x8593a3, steelDark: 0x2c3946, ground: 0x0a0e12, module: 0x1a242e, line: 0x3a4956,
    carriage: 0xc46a2c, accent: 0xe9843f, particle: 0xcfd9e3,
    hemiSky: 0xb9c7d6, hemiGround: 0x0c1116, envIntensity: 0.55, exposure: 1.0, additive: true, bloom: 0.55,
  },
  light: {
    bg: 0xf2f4f6, fogNear: 44, fogFar: 175, steel: 0x3e4a56, steelDark: 0x97a3ae, ground: 0xe6eaee, module: 0xd6dce2, line: 0xa4afba,
    carriage: 0xd8702b, accent: 0xd8702b, particle: 0x2b3642,
    hemiSky: 0xffffff, hemiGround: 0xb9c2ca, envIntensity: 0.9, exposure: 1.1, additive: false, bloom: 0.22,
  },
};
