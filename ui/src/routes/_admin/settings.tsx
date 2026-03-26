import { createFileRoute } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  useHeartbeatStatus,
  useHeartbeatEnable,
  useHeartbeatDisable,
  useGatewayStart,
  useGatewayStop,
  useGatewayRestart,
  useConfig,
  useUpdateConfig,
  useTtsStatus,
  useTtsEnable,
  useTtsDisable,
  useSttStatus,
} from "@/hooks/use-system";
import { getToken } from "@/lib/auth";
import {
  Heart,
  Server,
  Key,
  Play,
  Square,
  RotateCw,
  Mic,
  AudioLines,
  Smartphone,
  Save,
  CheckCircle2,
  XCircle,
} from "lucide-react";

export const Route = createFileRoute("/_admin/settings")({
  component: SettingsPage,
});

function SettingsPage() {
  const { data: heartbeat, isLoading: heartbeatLoading } = useHeartbeatStatus();
  const { data: config, isLoading: configLoading } = useConfig();
  const { data: ttsStatus, isLoading: ttsLoading } = useTtsStatus();
  const { data: sttStatus, isLoading: sttLoading } = useSttStatus();
  const enableHeartbeat = useHeartbeatEnable();
  const disableHeartbeat = useHeartbeatDisable();
  const gatewayStart = useGatewayStart();
  const gatewayStop = useGatewayStop();
  const gatewayRestart = useGatewayRestart();
  const updateConfig = useUpdateConfig();
  const ttsEnable = useTtsEnable();
  const ttsDisable = useTtsDisable();

  const token = getToken();

  const [ttsProvider, setTtsProvider] = useState("openai");
  const [ttsVoice, setTtsVoice] = useState("alloy");
  const [ttsModel, setTtsModel] = useState("tts-1");
  const [ttsAutoMode, setTtsAutoMode] = useState("off");
  const [sttProvider, setSttProvider] = useState("openai");
  const [sttModel, setSttModel] = useState("gpt-4o-mini-transcribe");

  useEffect(() => {
    if (!config) return;

    const media = (config.media ?? {}) as Record<string, unknown>;
    const tts = (media.tts ?? {}) as Record<string, unknown>;
    const stt = (media.stt ?? {}) as Record<string, unknown>;

    setTtsProvider(typeof tts.provider === "string" ? tts.provider : "openai");
    setTtsVoice(typeof tts.voice === "string" ? tts.voice : "alloy");
    setTtsModel(typeof tts.model === "string" ? tts.model : "tts-1");
    setTtsAutoMode(typeof tts.auto_mode === "string" ? tts.auto_mode : "off");
    setSttProvider(typeof stt.provider === "string" ? stt.provider : "openai");
    setSttModel(typeof stt.model === "string" ? stt.model : "gpt-4o-mini-transcribe");
  }, [config]);

  const handleHeartbeatToggle = (enabled: boolean) => {
    if (enabled) {
      enableHeartbeat.mutate(undefined, {
        onSuccess: () => toast.success("Heartbeat enabled"),
        onError: (error) => toast.error(error.message),
      });
    } else {
      disableHeartbeat.mutate(undefined, {
        onSuccess: () => toast.success("Heartbeat disabled"),
        onError: (error) => toast.error(error.message),
      });
    }
  };

  const saveMediaConfig = () => {
    if (!config) return;

    const nextConfig = structuredClone(config) as Record<string, unknown>;
    const media = (nextConfig.media ?? {}) as Record<string, unknown>;
    const currentTts = (media.tts ?? {}) as Record<string, unknown>;
    const currentStt = (media.stt ?? {}) as Record<string, unknown>;

    media.tts = {
      ...currentTts,
      provider: ttsProvider,
      voice: ttsVoice,
      model: ttsModel,
      auto_mode: ttsAutoMode,
    };

    media.stt = {
      ...currentStt,
      provider: sttProvider,
      model: sttModel,
    };

    nextConfig.media = media;

    updateConfig.mutate(nextConfig, {
      onSuccess: () => toast.success("Media settings saved"),
      onError: (error) => toast.error(error.message),
    });
  };

  const handleTtsToggle = (enabled: boolean) => {
    const mutation = enabled ? ttsEnable : ttsDisable;
    mutation.mutate(undefined, {
      onSuccess: () => toast.success(`TTS ${enabled ? "enabled" : "disabled"}`),
      onError: (error) => toast.error(error.message),
    });
  };

  return (
    <div className="space-y-8">
      <div>
        <h1 className="text-3xl font-bold tracking-tight">Settings</h1>
        <p className="mt-1 text-muted-foreground">Gateway configuration and controls</p>
      </div>

      <div className="grid gap-6 xl:grid-cols-[1.1fr,0.9fr]">
        <div className="space-y-6">
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2 text-base">
                <Heart className="h-4 w-4" />
                Heartbeat
              </CardTitle>
            </CardHeader>
            <CardContent>
              {heartbeatLoading ? (
                <Skeleton className="h-12 w-full" />
              ) : heartbeat ? (
                <div className="space-y-4">
                  <div className="flex items-center justify-between gap-4">
                    <div className="space-y-1">
                      <Label>Heartbeat Enabled</Label>
                      <p className="text-sm text-muted-foreground">
                        Send periodic heartbeat pings to keep sessions alive
                      </p>
                    </div>
                    <Switch
                      checked={heartbeat.enabled}
                      onCheckedChange={handleHeartbeatToggle}
                      disabled={enableHeartbeat.isPending || disableHeartbeat.isPending}
                    />
                  </div>
                  <Separator />
                  <dl className="space-y-3 text-sm">
                    <div className="flex justify-between">
                      <dt className="text-muted-foreground">Interval</dt>
                      <dd>{heartbeat.interval_seconds}s</dd>
                    </div>
                    <div className="flex justify-between gap-4">
                      <dt className="text-muted-foreground">Last Heartbeat</dt>
                      <dd className="text-right text-xs">
                        {heartbeat.last_heartbeat_at
                          ? new Date(heartbeat.last_heartbeat_at).toLocaleString()
                          : "Never"}
                      </dd>
                    </div>
                  </dl>
                </div>
              ) : null}
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2 text-base">
                <AudioLines className="h-4 w-4" />
                Text to Speech
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-6">
              {ttsLoading || configLoading ? (
                <Skeleton className="h-48 w-full" />
              ) : (
                <>
                  <div className="flex items-center justify-between gap-4 rounded-lg border p-4">
                    <div className="space-y-1">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium">Engine status</span>
                        <Badge variant={ttsStatus?.available ? "outline" : "secondary"}>
                          {ttsStatus?.available ? "Available" : "Not configured"}
                        </Badge>
                        <Badge variant={ttsStatus?.enabled ? "default" : "secondary"}>
                          {ttsStatus?.enabled ? "Enabled" : "Disabled"}
                        </Badge>
                      </div>
                      <p className="text-sm text-muted-foreground">
                        Configure provider defaults here, then toggle runtime synthesis.
                      </p>
                    </div>
                    <Switch
                      checked={ttsStatus?.enabled ?? false}
                      onCheckedChange={handleTtsToggle}
                      disabled={!ttsStatus?.available || ttsEnable.isPending || ttsDisable.isPending}
                    />
                  </div>

                  <div className="grid gap-4 md:grid-cols-2">
                    <div className="space-y-2">
                      <Label>Provider</Label>
                      <Select value={ttsProvider} onValueChange={setTtsProvider}>
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="openai">OpenAI</SelectItem>
                          <SelectItem value="elevenlabs">ElevenLabs</SelectItem>
                        </SelectContent>
                      </Select>
                    </div>
                    <div className="space-y-2">
                      <Label>Auto mode</Label>
                      <Select value={ttsAutoMode} onValueChange={setTtsAutoMode}>
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="off">Off</SelectItem>
                          <SelectItem value="always">Always</SelectItem>
                          <SelectItem value="inbound">Inbound audio only</SelectItem>
                          <SelectItem value="tagged">Tagged responses</SelectItem>
                        </SelectContent>
                      </Select>
                    </div>
                    <div className="space-y-2">
                      <Label>Voice</Label>
                      <Input value={ttsVoice} onChange={(e) => setTtsVoice(e.target.value)} />
                    </div>
                    <div className="space-y-2">
                      <Label>Model</Label>
                      <Input value={ttsModel} onChange={(e) => setTtsModel(e.target.value)} />
                    </div>
                  </div>

                  {ttsStatus?.voices?.length ? (
                    <div className="space-y-2 rounded-lg border p-4">
                      <p className="text-sm font-medium">Detected voices</p>
                      <div className="flex flex-wrap gap-2">
                        {ttsStatus.voices.map((voice) => (
                          <Badge key={voice.id} variant="outline" className="gap-1">
                            {voice.name}
                            {voice.language ? ` · ${voice.language}` : ""}
                          </Badge>
                        ))}
                      </div>
                    </div>
                  ) : null}
                </>
              )}
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2 text-base">
                <Mic className="h-4 w-4" />
                Speech to Text
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-6">
              {sttLoading || configLoading ? (
                <Skeleton className="h-40 w-full" />
              ) : (
                <>
                  <div className="flex items-center gap-2">
                    <Badge variant={sttStatus?.available ? "outline" : "secondary"}>
                      {sttStatus?.available ? "Available" : "Not configured"}
                    </Badge>
                    <Badge variant={sttStatus?.enabled ? "default" : "secondary"}>
                      {sttStatus?.enabled ? "Enabled" : "Disabled"}
                    </Badge>
                  </div>

                  <div className="grid gap-4 md:grid-cols-2">
                    <div className="space-y-2">
                      <Label>Provider</Label>
                      <Select value={sttProvider} onValueChange={setSttProvider}>
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="openai">OpenAI</SelectItem>
                        </SelectContent>
                      </Select>
                    </div>
                    <div className="space-y-2">
                      <Label>Model</Label>
                      <Input value={sttModel} onChange={(e) => setSttModel(e.target.value)} />
                    </div>
                  </div>
                </>
              )}
            </CardContent>
          </Card>
        </div>

        <div className="space-y-6">
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2 text-base">
                <Server className="h-4 w-4" />
                Gateway Control
              </CardTitle>
            </CardHeader>
            <CardContent>
              <div className="flex flex-wrap gap-3">
                <Button
                  variant="outline"
                  className="gap-2"
                  onClick={() => {
                    if (confirm("Start the gateway?")) gatewayStart.mutate();
                  }}
                  disabled={gatewayStart.isPending}
                >
                  <Play className="h-4 w-4" />
                  Start
                </Button>
                <Button
                  variant="outline"
                  className="gap-2"
                  onClick={() => {
                    if (confirm("Stop the gateway?")) gatewayStop.mutate();
                  }}
                  disabled={gatewayStop.isPending}
                >
                  <Square className="h-4 w-4" />
                  Stop
                </Button>
                <Button
                  variant="outline"
                  className="gap-2"
                  onClick={() => {
                    if (confirm("Restart the gateway?")) gatewayRestart.mutate();
                  }}
                  disabled={gatewayRestart.isPending}
                >
                  <RotateCw className="h-4 w-4" />
                  Restart
                </Button>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="flex flex-row items-center justify-between gap-3">
              <CardTitle className="flex items-center gap-2 text-base">
                <Smartphone className="h-4 w-4" />
                Device Pairing
              </CardTitle>
              <Badge variant="outline" className="gap-1">
                <CheckCircle2 className="h-3.5 w-3.5" />
                Backend ready
              </Badge>
            </CardHeader>
            <CardContent className="space-y-4 text-sm">
              <p className="text-muted-foreground">
                Pairing APIs are live. Use the mobile device pairing flow against the gateway to
                register trusted clients.
              </p>
              <div className="rounded-lg border p-4">
                <dl className="space-y-3">
                  <div className="flex items-center justify-between gap-4">
                    <dt className="text-muted-foreground">Status</dt>
                    <dd className="flex items-center gap-2">
                      <CheckCircle2 className="h-4 w-4 text-green-600" />
                      Available
                    </dd>
                  </div>
                  <div className="flex items-center justify-between gap-4">
                    <dt className="text-muted-foreground">Transport</dt>
                    <dd className="font-mono text-xs">Ed25519 challenge-response</dd>
                  </div>
                  <div className="flex items-center justify-between gap-4">
                    <dt className="text-muted-foreground">Token storage</dt>
                    <dd className="font-mono text-xs">SHA-256 persisted</dd>
                  </div>
                </dl>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="flex flex-row items-center justify-between gap-3">
              <CardTitle className="flex items-center gap-2 text-base">
                <Key className="h-4 w-4" />
                Connection
              </CardTitle>
              <Button
                size="sm"
                className="gap-2"
                onClick={saveMediaConfig}
                disabled={updateConfig.isPending || !config}
              >
                <Save className="h-4 w-4" />
                {updateConfig.isPending ? "Saving..." : "Save media settings"}
              </Button>
            </CardHeader>
            <CardContent>
              <dl className="space-y-3 text-sm">
                <div className="flex items-center justify-between gap-4">
                  <dt className="text-muted-foreground">API URL</dt>
                  <dd className="font-mono text-xs">{window.location.origin}</dd>
                </div>
                <div className="flex items-center justify-between gap-4">
                  <dt className="text-muted-foreground">Auth Token</dt>
                  <dd>
                    {token ? (
                      <Badge variant="outline" className="font-mono text-xs">
                        {token.slice(0, 4)}{"****"}{token.slice(-4)}
                      </Badge>
                    ) : (
                      <span className="inline-flex items-center gap-1 text-muted-foreground">
                        <XCircle className="h-4 w-4" />
                        Not set
                      </span>
                    )}
                  </dd>
                </div>
              </dl>
            </CardContent>
          </Card>
        </div>
      </div>
    </div>
  );
}
