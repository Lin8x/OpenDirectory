// Minimal typings for the remote desktop clients (neither ships its own).

declare module "@novnc/novnc" {
  export default class RFB extends EventTarget {
    constructor(target: HTMLElement, url: string | WebSocket, options?: { credentials?: { username?: string; password?: string; target?: string }; shared?: boolean; wsProtocols?: string[] });
    scaleViewport: boolean;
    resizeSession: boolean;
    clipViewport: boolean;
    viewOnly: boolean;
    focusOnClick: boolean;
    qualityLevel: number;
    compressionLevel: number;
    disconnect(): void;
    sendCredentials(c: { username?: string; password?: string; target?: string }): void;
    sendCtrlAltDel(): void;
    focus(): void;
  }
}

declare module "guacamole-common-js" {
  namespace Guacamole {
    class Status { code: number; message: string }
    class WebSocketTunnel { constructor(url: string) }
    class Display {
      getElement(): HTMLElement;
      getWidth(): number;
      getHeight(): number;
      scale(s: number): void;
      onresize: ((w: number, h: number) => void) | null;
    }
    class Client {
      constructor(tunnel: WebSocketTunnel);
      connect(data?: string): void;
      disconnect(): void;
      getDisplay(): Display;
      sendKeyEvent(pressed: 0 | 1, keysym: number): void;
      sendMouseState(state: unknown, applyDisplayScale?: boolean): void;
      sendSize(w: number, h: number): void;
      onstatechange: ((state: number) => void) | null;
      onerror: ((status: Status) => void) | null;
    }
    class Mouse {
      constructor(el: HTMLElement);
      onEach(events: string[], handler: (e: { state: unknown }) => void): void;
    }
    class Keyboard {
      constructor(el: HTMLElement | Document);
      onkeydown: ((keysym: number) => boolean | void) | null;
      onkeyup: ((keysym: number) => void) | null;
      reset(): void;
    }
  }
  export default Guacamole;
}
