// Premultiplied-alpha glass surface. Background must be supplied by the compositor,
// never by a screenshot of our own window (which would feed back into itself).
Texture2D<float4> Background : register(t0);
SamplerState LinearClamp : register(s0);
cbuffer Glass : register(b0) {
    float2 ViewSize;
    float2 Origin;
    float2 SurfaceSize;
    float Radius;
    float RefractionPixels;
    float BlurPixels;
    float TintOpacity;
    float2 DesktopOffset;
    float4 Tint;
};
float roundedDistance(float2 p) {
    float2 q=abs(p-SurfaceSize*.5)-(SurfaceSize*.5-Radius);
    return length(max(q,0))+min(max(q.x,q.y),0)-Radius;
}
float4 main(float4 position:SV_POSITION):SV_TARGET {
    float2 p=position.xy-Origin;
    float d=roundedDistance(p);
    float coverage=saturate(.5-d);
    if(coverage<=0) discard;
    float2 normal=normalize(float2(roundedDistance(p+float2(.5,0))-roundedDistance(p-float2(.5,0)),roundedDistance(p+float2(0,.5))-roundedDistance(p-float2(0,.5)))+1e-6);
    float edge=exp(-abs(d)/max(Radius*.45,1));
    float2 uv=(position.xy+DesktopOffset-normal*RefractionPixels*edge)/ViewSize;
    float2 step=BlurPixels/ViewSize;
    float3 color=Background.Sample(LinearClamp,uv).rgb*.4;
    color+=Background.Sample(LinearClamp,uv+float2(step.x,0)).rgb*.15;
    color+=Background.Sample(LinearClamp,uv-float2(step.x,0)).rgb*.15;
    color+=Background.Sample(LinearClamp,uv+float2(0,step.y)).rgb*.15;
    color+=Background.Sample(LinearClamp,uv-float2(0,step.y)).rgb*.15;
    color=lerp(color,Tint.rgb,saturate(TintOpacity));
    float rim=exp(-abs(d+1)*1.5);
    color+=rim*(.10+.12*saturate(dot(normal,normalize(float2(-.6,-.8)))));
    return float4(saturate(color)*coverage,coverage);
}
float4 vertex(uint id:SV_VertexID):SV_POSITION {
    float2 p=float2((id<<1)&2,id&2);
    return float4(p*float2(2,-2)+float2(-1,1),0,1);
}
