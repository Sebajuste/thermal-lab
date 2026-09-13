interface Props {
  values: (number | null)[];
  /** Bornes figées quand on veut comparer deux courbes à la même échelle. */
  min?: number;
  max?: number;
  color?: string;
  height?: number;
}

/** Courbe minimaliste en SVG : pas de dépendance de graphes pour un POC. */
export default function Sparkline({
  values,
  min,
  max,
  color = "#4fc3ff",
  height = 30,
}: Props) {
  const points = values.filter((v): v is number => v !== null && Number.isFinite(v));
  if (points.length < 2) {
    return <div className="spark-empty" style={{ height }} />;
  }

  const lo = min ?? Math.min(...points);
  const hi = max ?? Math.max(...points);
  const span = hi - lo || 1;
  const width = 100;

  const path = points
    .map((v, i) => {
      const x = (i / (points.length - 1)) * width;
      const y = height - ((v - lo) / span) * height;
      return `${i === 0 ? "M" : "L"}${x.toFixed(2)},${y.toFixed(2)}`;
    })
    .join(" ");

  return (
    <svg
      className="spark"
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      style={{ height }}
    >
      <path
        d={path}
        fill="none"
        stroke={color}
        strokeWidth="1.5"
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}
