interface VeloceLogoProps {
  size?: number;
}

export function VeloceLogo({ size = 36 }: VeloceLogoProps) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width={size}
      height={size}
      viewBox="0 0 512 512"
      fill="none"
      aria-hidden="true"
    >
      <defs>
        <linearGradient
          id="veloceLogoBg"
          x1="120"
          y1="0"
          x2="392"
          y2="512"
          gradientUnits="userSpaceOnUse"
        >
          <stop offset="0%" stopColor="#171C26" />
          <stop offset="100%" stopColor="#0B0E14" />
        </linearGradient>
        <linearGradient
          id="veloceLogoBorder"
          x1="256"
          y1="8"
          x2="256"
          y2="504"
          gradientUnits="userSpaceOnUse"
        >
          <stop offset="0%" stopColor="#FFFFFF" stopOpacity="0.16" />
          <stop offset="100%" stopColor="#FFFFFF" stopOpacity="0.03" />
        </linearGradient>
      </defs>
      <rect x="8" y="8" width="496" height="496" rx="112" fill="url(#veloceLogoBg)" />
      <rect
        x="9"
        y="9"
        width="494"
        height="494"
        rx="111"
        stroke="url(#veloceLogoBorder)"
        strokeWidth="2"
      />
      <path
        d="M 244 382 C 296 322 350 236 412 126 C 396 222 350 310 278 378 C 264 390 250 390 244 382 Z"
        fill="#CBD5E1"
      />
      <path
        d="M 96 138 C 156 150 210 194 250 266 C 266 226 292 196 324 182 C 344 174 362 178 374 194 C 358 190 344 194 334 206 C 312 246 288 312 254 378 C 248 390 232 390 226 378 C 196 322 160 268 112 222 C 142 226 166 234 188 248 C 146 206 116 170 96 138 Z"
        fill="#F8FAFC"
      />
    </svg>
  );
}
