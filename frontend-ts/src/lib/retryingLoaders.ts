// THREE.Loader subclasses (TextureLoader, OBJLoader) issue their own XHR/fetch internally and
// have no retry/backoff of their own — a single dropped request permanently fails a texture or
// mesh with no recovery. These wrappers retry the underlying load with the same backoff policy
// as fetchWithRetry, so asset loads get the same resilience as JSON/blob API calls.
import * as THREE from 'three'
import { OBJLoader } from 'three/examples/jsm/loaders/OBJLoader.js'
import { MAX_RETRIES, backoffDelayMs } from './fetchWithRetry'

type ErrorEventLike = ErrorEvent | Error | unknown

function retryingLoad<T>(
  attemptLoad: (onLoad: (data: T) => void, onError: (err: ErrorEventLike) => void) => void,
  onLoad: (data: T) => void,
  onError: ((err: ErrorEventLike) => void) | undefined,
  isAborted: () => boolean,
): void {
  const attempt = (n: number) => {
    if (isAborted()) return
    attemptLoad(onLoad, err => {
      if (isAborted()) return
      if (n >= MAX_RETRIES) { onError?.(err); return }
      setTimeout(() => attempt(n + 1), backoffDelayMs(n))
    })
  }
  attempt(0)
}

/**
 * Drop-in replacement for THREE.TextureLoader that retries failed loads with exponential
 * backoff before giving up. Set `isAborted` before calling `load()` to stop retrying once
 * the caller (e.g. an unmounted component) no longer cares about the result.
 */
export class RetryingTextureLoader extends THREE.TextureLoader {
  isAborted: () => boolean = () => false

  override load(
    url: string,
    onLoad?: (texture: THREE.Texture<HTMLImageElement>) => void,
    onProgress?: (event: ProgressEvent) => void,
    onError?: (err: ErrorEventLike) => void,
  ): THREE.Texture<HTMLImageElement> {
    const texture = new THREE.Texture<HTMLImageElement>()
    retryingLoad<THREE.Texture<HTMLImageElement>>(
      (resolve, reject) => super.load(url, resolve, onProgress, reject),
      loaded => {
        texture.image = loaded.image
        texture.needsUpdate = true
        onLoad?.(texture)
      },
      onError,
      this.isAborted,
    )
    return texture
  }
}

/**
 * Drop-in replacement for OBJLoader that retries failed loads with exponential backoff before
 * giving up. Set `isAborted` before calling `load()` to stop retrying once the caller no longer
 * cares about the result.
 */
export class RetryingOBJLoader extends OBJLoader {
  isAborted: () => boolean = () => false

  override load(
    url: string,
    onLoad: (group: THREE.Group) => void,
    onProgress?: (event: ProgressEvent) => void,
    onError?: (err: ErrorEventLike) => void,
  ): void {
    retryingLoad<THREE.Group>(
      (resolve, reject) => super.load(url, resolve, onProgress, reject),
      onLoad,
      onError,
      this.isAborted,
    )
  }
}
