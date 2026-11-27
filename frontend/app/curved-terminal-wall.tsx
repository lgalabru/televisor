"use client";

import { useEffect, useRef, useState } from "react";

export interface TerminalScreenData {
  id: string;
  label: string;
  command: string;
  lines: string[];
  state: string;
}

interface CurvedTerminalWallProps {
  screens: TerminalScreenData[];
}

const COLUMN_COUNT = 12;
const ROW_COUNT = 3;
const TRACK_PAGE_COUNT = COLUMN_COUNT * 3;
const INITIAL_PAGE = COLUMN_COUNT;
const TEXTURE_WIDTH = 6144;
const TEXTURE_HEIGHT = 820;

function roundedRect(
  context: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  radius: number,
) {
  context.beginPath();
  context.roundRect(x, y, width, height, radius);
}

function drawTexture(screens: TerminalScreenData[]) {
  const textureCanvas = document.createElement("canvas");
  textureCanvas.width = TEXTURE_WIDTH;
  textureCanvas.height = TEXTURE_HEIGHT;

  const context = textureCanvas.getContext("2d");
  if (!context) return textureCanvas;

  context.clearRect(0, 0, TEXTURE_WIDTH, TEXTURE_HEIGHT);

  context.fillStyle = "#151515";
  context.fillRect(0, 0, TEXTURE_WIDTH, TEXTURE_HEIGHT);
  context.fillStyle = "#474747";
  context.fillRect(0, 0, TEXTURE_WIDTH, 3);
  context.fillRect(0, TEXTURE_HEIGHT - 3, TEXTURE_WIDTH, 3);

  context.fillStyle = "#080808";
  context.fillRect(18, 18, TEXTURE_WIDTH - 36, TEXTURE_HEIGHT - 36);

  const columnWidth = TEXTURE_WIDTH / COLUMN_COUNT;
  context.textBaseline = "middle";
  for (let column = 0; column < COLUMN_COUNT; column += 1) {
    const center = column * columnWidth + columnWidth / 2;
    context.fillStyle = "#f12b35";
    context.fillRect(center - 86, 17, 172, 3);
    context.fillStyle = "#77746e";
    context.font = "700 10px ui-monospace, SFMono-Regular, Consolas, monospace";
    context.textAlign = "center";
    context.fillText(
      `TERMINAL COLUMN ${String(column + 1).padStart(2, "0")}`,
      center,
      35,
    );
  }

  const gridTop = 52;
  const gridBottom = 60;
  const rowGap = 14;
  const cardWidth = columnWidth - 22;
  const cardHeight =
    (TEXTURE_HEIGHT - gridTop - gridBottom - rowGap * (ROW_COUNT - 1)) /
    ROW_COUNT;

  Array.from({ length: COLUMN_COUNT * ROW_COUNT }, (_, index) => {
    const column = Math.floor(index / ROW_COUNT);
    const row = index % ROW_COUNT;
    const screen = screens[index % screens.length];
    const x = column * columnWidth + 11;
    const y = gridTop + row * (cardHeight + rowGap);
    const active = row === 1;

    roundedRect(context, x, y, cardWidth, cardHeight, 18);
    context.fillStyle = "#101010";
    context.fill();
    context.lineWidth = active ? 3 : 2;
    context.strokeStyle = active ? "#f12b35" : "#343434";
    context.stroke();

    context.fillStyle = "#181818";
    context.fillRect(x + 2, y + 2, cardWidth - 4, 34);
    context.fillStyle = "#696762";
    context.font = "700 12px ui-monospace, SFMono-Regular, Consolas, monospace";
    context.textAlign = "left";
    context.fillText(`${screen.id} / ${screen.label}`, x + 16, y + 19);
    context.fillStyle = screen.state === "LIVE" ? "#f12b35" : "#85e89d";
    context.textAlign = "right";
    context.fillText(screen.state, x + cardWidth - 16, y + 19);

    context.textAlign = "left";
    context.fillStyle = "#85e89d";
    context.font = "700 17px ui-monospace, SFMono-Regular, Consolas, monospace";
    context.fillText("❯", x + 16, y + 67);
    context.fillStyle = "#e0ddd6";
    context.font = "600 15px ui-monospace, SFMono-Regular, Consolas, monospace";
    context.fillText(`televisor ${screen.command}`, x + 39, y + 67, cardWidth - 58);

    context.fillStyle = "#292929";
    context.fillRect(x + 16, y + 86, cardWidth - 32, 2);

    context.fillStyle = "#8b8983";
    context.font = "500 13px ui-monospace, SFMono-Regular, Consolas, monospace";
    screen.lines.forEach((line, lineIndex) => {
      context.fillText(line, x + 16, y + 116 + lineIndex * 22, cardWidth - 32);
    });

    context.fillStyle = "#171717";
    context.fillRect(x + 2, y + cardHeight - 28, cardWidth - 4, 26);
    context.fillStyle = "#85e89d";
    context.beginPath();
    context.arc(x + 17, y + cardHeight - 15, 4, 0, Math.PI * 2);
    context.fill();
    context.fillStyle = "#696762";
    context.font = "500 9px ui-monospace, SFMono-Regular, Consolas, monospace";
    context.fillText("SIGNAL 100%", x + 29, y + cardHeight - 15);
  });

  const consoleY = TEXTURE_HEIGHT - 47;
  for (let column = 0; column < COLUMN_COUNT; column += 1) {
    const x = column * columnWidth + 11;
    roundedRect(context, x, consoleY, cardWidth, 30, 7);
    context.fillStyle = "#121212";
    context.fill();
    context.strokeStyle = "#303030";
    context.lineWidth = 2;
    context.stroke();
    context.font = "500 9px ui-monospace, SFMono-Regular, Consolas, monospace";
    context.fillStyle = "#85e89d";
    context.textAlign = "left";
    context.fillText("●  12 STREAMS ONLINE", x + 14, consoleY + 15);
    context.fillStyle = "#77746e";
    context.textAlign = "right";
    context.fillText("LOCAL / REDACTED", x + cardWidth - 14, consoleY + 15);
  }

  return textureCanvas;
}

export function CurvedTerminalWall({ screens }: CurvedTerminalWallProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const [ready, setReady] = useState(false);
  const [activeColumn, setActiveColumn] = useState(0);

  useEffect(() => {
    let cancelled = false;
    let disposeScene: (() => void) | undefined;

    void import("three").then((THREE) => {
      if (cancelled) return;
      const container = containerRef.current;
      const canvas = canvasRef.current;
      const scroller = scrollRef.current;
      if (!container || !canvas || !scroller) return;

      const scene = new THREE.Scene();
      const camera = new THREE.PerspectiveCamera(50, 1, 0.1, 50);
      const renderer = new THREE.WebGLRenderer({
        canvas,
        alpha: true,
        antialias: true,
        powerPreference: "high-performance",
      });
      renderer.outputColorSpace = THREE.SRGBColorSpace;
      renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));

      const sourceCanvas = drawTexture(screens);
      const texture = new THREE.CanvasTexture(sourceCanvas);
      texture.colorSpace = THREE.SRGBColorSpace;
      texture.anisotropy = Math.min(renderer.capabilities.getMaxAnisotropy(), 8);

      const radius = 3.2;
      const height = 2.15;
      const columnTurn = (Math.PI * 2) / COLUMN_COUNT;
      const panelWidth = 2 * radius * Math.tan(columnTurn / 2) * 0.94;
      const material = new THREE.MeshBasicMaterial({
        map: texture,
        transparent: true,
      });
      const wall = new THREE.Group();
      const panelGeometries: InstanceType<typeof THREE.PlaneGeometry>[] = [];
      const panels: InstanceType<typeof THREE.Mesh>[] = [];

      for (let column = 0; column < COLUMN_COUNT; column += 1) {
        const geometry = new THREE.PlaneGeometry(panelWidth, height);
        const uvs = geometry.getAttribute("uv");
        for (let index = 0; index < uvs.count; index += 1) {
          uvs.setX(index, (column + uvs.getX(index)) / COLUMN_COUNT);
        }
        uvs.needsUpdate = true;
        panelGeometries.push(geometry);

        const panel = new THREE.Mesh(geometry, material);
        const angle = column * columnTurn;
        panel.position.set(
          radius * Math.sin(angle),
          0,
          -radius * Math.cos(angle),
        );
        panel.lookAt(0, 0, 0);
        wall.add(panel);
        panels.push(panel);
      }
      scene.add(wall);

      let initialized = false;
      let previousWidth = 0;
      let snapTimeout: ReturnType<typeof setTimeout> | undefined;
      let lastActiveColumn = -1;
      const renderScene = () => {
        renderer.render(scene, camera);
      };

      const updateRotation = () => {
        const page = scroller.scrollLeft / Math.max(container.clientWidth, 1);
        wall.rotation.y = -page * columnTurn;
        panels.forEach((panel, column) => {
          const rawDelta = column - page;
          const wrappedDelta =
            ((rawDelta + COLUMN_COUNT / 2) % COLUMN_COUNT + COLUMN_COUNT) %
              COLUMN_COUNT -
            COLUMN_COUNT / 2;
          const focus = Math.max(0, 1 - Math.abs(wrappedDelta));
          const panelRadius = radius - focus * 0.85;
          const angle = column * columnTurn;
          panel.position.set(
            panelRadius * Math.sin(angle),
            0,
            -panelRadius * Math.cos(angle),
          );
        });
        const nextActive =
          ((Math.round(page) % COLUMN_COUNT) + COLUMN_COUNT) % COLUMN_COUNT;
        if (nextActive !== lastActiveColumn) {
          lastActiveColumn = nextActive;
          setActiveColumn(nextActive);
        }
        renderScene();
      };

      const resize = () => {
        const rect = container.getBoundingClientRect();
        if (rect.width === 0 || rect.height === 0) return;
        const page =
          previousWidth > 0 ? scroller.scrollLeft / previousWidth : INITIAL_PAGE;
        renderer.setSize(rect.width, rect.height, false);
        camera.aspect = (rect.width / rect.height) * 0.8;
        camera.position.set(0, 0, 0);
        camera.lookAt(0, 0, -radius);
        camera.updateProjectionMatrix();
        previousWidth = rect.width;
        scroller.scrollLeft = page * rect.width;
        updateRotation();
        setReady(true);
      };

      const wrapScrollPosition = () => {
        const width = Math.max(container.clientWidth, 1);
        const page = Math.round(scroller.scrollLeft / width);
        if (page <= 2) scroller.scrollLeft = (page + COLUMN_COUNT) * width;
        if (page >= TRACK_PAGE_COUNT - 3) {
          scroller.scrollLeft = (page - COLUMN_COUNT) * width;
        }
        updateRotation();
      };

      const handleScroll = () => {
        updateRotation();
        if (snapTimeout) clearTimeout(snapTimeout);
        snapTimeout = setTimeout(wrapScrollPosition, 180);
      };

      const handleWheel = (event: WheelEvent) => {
        if (Math.abs(event.deltaY) <= Math.abs(event.deltaX)) return;
        event.preventDefault();
        scroller.scrollLeft += event.deltaY;
      };

      const resizeObserver = new ResizeObserver(resize);
      resizeObserver.observe(container);
      scroller.addEventListener("scroll", handleScroll, { passive: true });
      scroller.addEventListener("wheel", handleWheel, { passive: false });
      resize();
      if (!initialized) {
        scroller.scrollLeft = container.clientWidth * INITIAL_PAGE;
        initialized = true;
        updateRotation();
      }

      disposeScene = () => {
        if (snapTimeout) clearTimeout(snapTimeout);
        resizeObserver.disconnect();
        scroller.removeEventListener("scroll", handleScroll);
        scroller.removeEventListener("wheel", handleWheel);
        scene.remove(wall);
        panelGeometries.forEach((geometry) => geometry.dispose());
        material.dispose();
        texture.dispose();
        renderer.dispose();
      };
    }).catch(() => {
      if (!cancelled) setReady(false);
    });

    return () => {
      cancelled = true;
      disposeScene?.();
    };
  }, [screens]);

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    const scroller = scrollRef.current;
    if (!scroller) return;
    event.preventDefault();
    const direction = event.key === "ArrowRight" ? 1 : -1;
    const page = Math.round(scroller.scrollLeft / scroller.clientWidth) + direction;
    scroller.scrollTo({ left: page * scroller.clientWidth, behavior: "smooth" });
  };

  return (
    <div className={`webgl-wall${ready ? " is-ready" : ""}`} ref={containerRef}>
      <div className="webgl-fallback" aria-hidden="true">
        {screens.map((screen) => (
          <article key={screen.id}>
            <span>{screen.id} / {screen.label}</span>
            <code>❯ televisor {screen.command}</code>
          </article>
        ))}
      </div>
      <canvas
        ref={canvasRef}
        role="img"
        aria-label={`A 360-degree terminal cylinder with column ${activeColumn + 1} of ${COLUMN_COUNT} centered`}
      />
      <div
        ref={scrollRef}
        className="wall-scroll"
        role="region"
        tabIndex={0}
        aria-label="Rotate the terminal wall. Scroll horizontally or use the left and right arrow keys."
        onKeyDown={handleKeyDown}
      >
        <div className="wall-scroll-track" aria-hidden="true">
          {Array.from({ length: TRACK_PAGE_COUNT }, (_, index) => (
            <span key={index} />
          ))}
        </div>
      </div>
      <div className="wall-rotation-status" aria-hidden="true">
        <span>SCROLL TO ROTATE</span>
        <b>{String(activeColumn + 1).padStart(2, "0")} / 12</b>
      </div>
    </div>
  );
}
