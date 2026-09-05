import * as THREE from 'three';
import { EffectComposer } from 'three/examples/jsm/postprocessing/EffectComposer.js';
import { RenderPass } from 'three/examples/jsm/postprocessing/RenderPass.js';
import { UnrealBloomPass } from 'three/examples/jsm/postprocessing/UnrealBloomPass.js';
import { OutputPass } from 'three/examples/jsm/postprocessing/OutputPass.js';

export interface Post { composer: EffectComposer; bloom: UnrealBloomPass; setSize(w: number, h: number, dpr: number): void; dispose(): void }

/** High tier only: a restrained bloom so the carriage lamp, the LEDs and the accent pulses read as light sources. */
export function createPost(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera, strength: number): Post {
  const composer = new EffectComposer(renderer);
  composer.addPass(new RenderPass(scene, camera));
  const bloom = new UnrealBloomPass(new THREE.Vector2(1, 1), strength, 0.4, 0.92);
  composer.addPass(bloom);
  composer.addPass(new OutputPass());
  return {
    composer, bloom,
    setSize(w, h, dpr) { composer.setPixelRatio(dpr); composer.setSize(w, h); },
    dispose() { composer.dispose(); },
  };
}
