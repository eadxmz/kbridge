// ============================================================================
// KBridge v0.6.1 — Intel HD Graphics 4400 Dual-GPU Offload Frame Gen Shaders
// Modes:
//   1. Pyramid Optical Flow (2-level Coarse 1/8 -> Fine 1/4 Block Matching + Smooth)
//   2. Ultra-Lite Lucas-Kanade (Single-pass PS 4.0 Gradient Optical Flow)
//   3. Adaptive Video Smoother (Scene-cut aware 24/30 -> 60 fps temporal warp)
// Latency:
//   ZeroLag-X Predictive Extrapolation (0-frame hold + mouse-delta camera vector)
//   vs Classic Bidirectional Interpolation
// ============================================================================

cbuffer FrameGenParams : register(b0) {
    float2 texelSize;       // 1.0 / full_width, 1.0 / full_height
    float2 flowTexelSize;   // 1.0 / flow_width, 1.0 / flow_height
    float2 mouseBiasUv;     // Live mouse-delta camera-pan vector in UV space (ZeroLag-X)
    float  alpha;           // Synthesis factor (0.5 for 2x midpoint)
    float  mode;            // 0 = Pyramid, 1 = Lucas-Kanade, 2 = Video Smoother
    float  zeroLagExtrap;   // 1.0 = ZeroLag-X Forward Extrapolation, 0.0 = Classic Interpolation
    float  hudThreshold;    // Static HUD luma diff threshold (~0.015)
    float  sceneCutThresh;  // Scene-cut fallback threshold
    float  _pad;
};

Texture2D texPrev   : register(t0);
Texture2D texCurr   : register(t1);
Texture2D texFlow   : register(t2);
SamplerState samLin : register(s0);

struct VSOut {
    float4 pos : SV_POSITION;
    float2 uv  : TEXCOORD0;
};

// Fullscreen triangle without any vertex buffer
VSOut VS_Fullscreen(uint id : SV_VertexID) {
    VSOut o;
    float2 uv = float2((id << 1) & 2, id & 2);
    o.uv  = uv;
    o.pos = float4(uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return o;
}

float Luma(float3 rgb) {
    return dot(rgb, float3(0.299, 0.587, 0.114));
}

// Pass 0: Downsample RGB frame to Luma (R16F)
float PS_LumaDownsample(VSOut i) : SV_TARGET {
    float3 c = texCurr.Sample(samLin, i.uv).rgb;
    return Luma(c);
}

// Pass 1 (Pyramid Level 0 — 1/8 res): Coarse 5x5 Block Matching (SAD)
float2 PS_PyramidCoarse(VSOut i) : SV_TARGET {
    float2 bestMv = float2(0.0, 0.0);
    float bestCost = 1e9;
    float2 stepUv = flowTexelSize;

    [unroll]
    for (int dy = -2; dy <= 2; dy++) {
        [unroll]
        for (int dx = -2; dx <= 2; dx++) {
            float2 candMv = float2(dx, dy) * stepUv;
            float cost = 0.0;
            [unroll]
            for (int py = -1; py <= 1; py++) {
                [unroll]
                for (int px = -1; px <= 1; px++) {
                    float2 pOff = float2(px, py) * stepUv;
                    float lCurr = texCurr.Sample(samLin, i.uv + pOff).r;
                    float lPrev = texPrev.Sample(samLin, i.uv + pOff - candMv).r;
                    cost += abs(lCurr - lPrev);
                }
            }
            cost += length(candMv) * 0.15;
            if (cost < bestCost) {
                bestCost = cost;
                bestMv = candMv;
            }
        }
    }
    return bestMv;
}

// Pass 2 (Pyramid Level 1 — 1/4 res): Fine 3x3 Refinement around upsampled coarse MV
float2 PS_PyramidFine(VSOut i) : SV_TARGET {
    float2 baseMv = texFlow.Sample(samLin, i.uv).rg;
    float2 bestMv = baseMv;
    float bestCost = 1e9;
    float2 stepUv = flowTexelSize;

    [unroll]
    for (int dy = -1; dy <= 1; dy++) {
        [unroll]
        for (int dx = -1; dx <= 1; dx++) {
            float2 candMv = baseMv + float2(dx, dy) * stepUv;
            float cost = 0.0;
            [unroll]
            for (int py = -1; py <= 1; py++) {
                [unroll]
                for (int px = -1; px <= 1; px++) {
                    float2 pOff = float2(px, py) * stepUv;
                    float lCurr = texCurr.Sample(samLin, i.uv + pOff).r;
                    float lPrev = texPrev.Sample(samLin, i.uv + pOff - candMv).r;
                    cost += abs(lCurr - lPrev);
                }
            }
            cost += length(candMv - baseMv) * 0.10;
            if (cost < bestCost) {
                bestCost = cost;
                bestMv = candMv;
            }
        }
    }
    return bestMv;
}

// Pass 2b: 3x3 Motion Vector Smoothing (Edge-preserving weighted filter to kill jitter)
float2 PS_SmoothFlow(VSOut i) : SV_TARGET {
    float2 center = texFlow.Sample(samLin, i.uv).rg;
    float2 acc = center * 4.0;
    float wSum = 4.0;

    [unroll]
    for (int dy = -1; dy <= 1; dy++) {
        [unroll]
        for (int dx = -1; dx <= 1; dx++) {
            if (dx == 0 && dy == 0) continue;
            float2 n = texFlow.Sample(samLin, i.uv + float2(dx, dy) * flowTexelSize).rg;
            float dist = length(n - center);
            float w = exp(-dist * 120.0);
            acc += n * w;
            wSum += w;
        }
    }
    return acc / wSum;
}

// Mode 2 & Mode 3: Ultra-Lite Single-Pass Lucas-Kanade Optical Flow (PS 4.0)
float2 PS_LucasKanade(VSOut i) : SV_TARGET {
    float2 s = flowTexelSize;
    float sIx2 = 0.0, sIy2 = 0.0, sIxIy = 0.0, sIxIt = 0.0, sIyIt = 0.0;

    [unroll]
    for (int dy = -1; dy <= 1; dy++) {
        [unroll]
        for (int dx = -1; dx <= 1; dx++) {
            float2 uv = i.uv + float2(dx, dy) * s;
            float cR = texCurr.Sample(samLin, uv + float2(s.x, 0.0)).r;
            float cL = texCurr.Sample(samLin, uv - float2(s.x, 0.0)).r;
            float cD = texCurr.Sample(samLin, uv + float2(0.0, s.y)).r;
            float cU = texCurr.Sample(samLin, uv - float2(0.0, s.y)).r;
            float c0 = texCurr.Sample(samLin, uv).r;
            float p0 = texPrev.Sample(samLin, uv).r;

            float Ix = (cR - cL) * 0.5;
            float Iy = (cD - cU) * 0.5;
            float It = c0 - p0;

            sIx2  += Ix * Ix;
            sIy2  += Iy * Iy;
            sIxIy += Ix * Iy;
            sIxIt += Ix * It;
            sIyIt += Iy * It;
        }
    }

    float eps = 0.0025;
    float a = sIx2 + eps;
    float d = sIy2 + eps;
    float b = sIxIy;
    float det = a * d - b * b;

    float2 mv = float2(0.0, 0.0);
    if (abs(det) > 1e-6) {
        mv.x = (-d * sIxIt + b * sIyIt) / det;
        mv.y = ( b * sIxIt - a * sIyIt) / det;
        mv *= s * 1.5;
    }

    float maxLen = (mode > 1.5) ? 0.025 : 0.045;
    float len = length(mv);
    if (len > maxLen) {
        mv = mv * (maxLen / len);
    }
    return mv;
}

// Pass 3a: Passthrough real frame (Zero-Hold immediate presentation)
float4 PS_Passthrough(VSOut i) : SV_TARGET {
    return float4(texCurr.Sample(samLin, i.uv).rgb, 1.0);
}

// Pass 3b: Final Frame Synthesis (ZeroLag-X Predictive Extrapolation OR Classic Interpolation)
float4 PS_Synthesize(VSOut i) : SV_TARGET {
    float3 rawCurr = texCurr.Sample(samLin, i.uv).rgb;
    float3 rawPrev = texPrev.Sample(samLin, i.uv).rgb;
    float staticDiff = abs(Luma(rawCurr) - Luma(rawPrev));

    if (staticDiff < hudThreshold && length(mouseBiasUv) < 0.0005) {
        return float4(rawCurr, 1.0);
    }

    if (mode > 1.5 && staticDiff > sceneCutThresh) {
        return float4(rawCurr, 1.0);
    }

    float2 mv = texFlow.Sample(samLin, i.uv).rg;

    if (zeroLagExtrap > 0.5) {
        float hudProtect = smoothstep(hudThreshold, hudThreshold * 3.0, staticDiff);
        float2 predMv = mv + mouseBiasUv * hudProtect;

        float2 uvFwd = saturate(i.uv - predMv * alpha);
        float3 warpedCurr = texCurr.Sample(samLin, uvFwd).rgb;

        float2 uvBackCheck = saturate(i.uv - predMv * (1.0 + alpha));
        float3 backPrev = texPrev.Sample(samLin, uvBackCheck).rgb;
        float consistencyErr = abs(Luma(warpedCurr) - Luma(backPrev));

        float disoccFallback = smoothstep(0.08, 0.28, consistencyErr);
        float3 outRgb = lerp(warpedCurr, rawCurr, disoccFallback * 0.75);
        return float4(outRgb, 1.0);
    } else {
        float2 uvPrev = saturate(i.uv + mv * alpha);
        float2 uvCurr = saturate(i.uv - mv * (1.0 - alpha));
        float3 cPrev = texPrev.Sample(samLin, uvPrev).rgb;
        float3 cCurr = texCurr.Sample(samLin, uvCurr).rgb;

        float mismatch = abs(Luma(cPrev) - Luma(cCurr));
        float3 blended = lerp(cPrev, cCurr, alpha);

        float fall = smoothstep(0.12, 0.35, mismatch);
        float3 outRgb = lerp(blended, rawCurr, fall * 0.65);
        return float4(outRgb, 1.0);
    }
}
