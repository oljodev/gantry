import './styles/tokens.css';
import './styles/fonts.css';
import './styles/styles.css';
import { initReleases } from './releases';
import { markVisitorOs } from './os';
import { initConnectorFilters } from './connectors/grid';
import { initReveal } from './reveal';
import { detectTier, onReducedMotionChange } from './tiers';
import { currentTheme, onThemeChange, applyThemeOverride } from './theme';

document.documentElement.classList.add('js');
applyThemeOverride();
markVisitorOs();
void initReleases();
initConnectorFilters();
initReveal();
void bootScene();

/** The scene is progressive enhancement. The poster, the copy and the buttons are usable before it loads and if it never does. */
async function bootScene(): Promise<void> {
  const host = document.querySelector<HTMLElement>('[data-scene]');
  const canvas = host?.querySelector<HTMLCanvasElement>('canvas');
  const caption = host?.querySelector<HTMLElement>('[data-caption]');
  if (!host || !canvas || !caption) return;

  const tier = detectTier();
  if (tier === 'poster') return;

  const start = async () => {
    try {
      const { createScene } = await import('./scene/index');
      const handle = createScene(canvas, {
        tier,
        theme: currentTheme(),
        captionEl: caption,
        onFirstFrame: () => { host.dataset.sceneState = 'live'; },
        onDegrade: (to) => {
          if (to === 'poster') { host.dataset.sceneState = 'poster'; setTimeout(() => handle.dispose(), 800); }
        },
      });

      const io = new IntersectionObserver(([e]) => { e?.isIntersecting ? handle.start() : handle.stop(); }, { threshold: 0.05 });
      io.observe(host);
      document.addEventListener('visibilitychange', () => { document.hidden ? handle.stop() : handle.start(); });
      onThemeChange((mode) => handle.setTheme(mode));
      onReducedMotionChange((reduced) => {
        if (reduced) { host.dataset.sceneState = 'poster'; io.disconnect(); setTimeout(() => handle.dispose(), 800); }
      });
    } catch {
      host.dataset.sceneState = 'poster';
    }
  };

  if ('requestIdleCallback' in window) window.requestIdleCallback(() => void start(), { timeout: 1500 });
  else setTimeout(() => void start(), 300);
}
