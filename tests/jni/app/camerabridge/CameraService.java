package app.camerabridge;
import java.nio.ByteBuffer;
import java.util.Arrays;

/** Runs real Rust JNI with a deterministic Android platform fixture. */
public final class CameraService {
    private boolean armed=true;
    private int connects, failures, disconnects, writes;
    private boolean nativeArmed() { return armed; }
    private void nativeConnect() throws Exception { if (++connects==1) throw new java.io.IOException("fixture reconnect"); }
    private byte[] nativeStatus() { return "{}".getBytes(java.nio.charset.StandardCharsets.UTF_8); }
    private CameraEngine.Packet nativePoll() { return new CameraEngine.Packet(new byte[]{7,8,9},90); }
    private void nativeWrite(byte[] packet) {
        ByteBuffer b=ByteBuffer.wrap(packet);
        int size=b.getInt(), rotation=b.getInt();
        if (size!=packet.length-8 || (rotation!=-1 && rotation!=90)) throw new AssertionError("Bad framing");
        writes++;
    }
    private void nativeTick() { armed=false; }
    private void nativeFailure(String message) { if (!message.contains("fixture reconnect")) throw new AssertionError(message); failures++; }
    private void nativeDisconnect() { disconnects++; }
    private static void check(boolean ok,String message) { if (!ok) throw new AssertionError(message); }
    public static void main(String[] args) {
        check(NativeBridge.chooseFps(new int[]{5,30,30,30},30,1)==1,"Native FPS selection");
        long q=NativeBridge.queueCreate();
        NativeBridge.queueConfig(q,new byte[]{4,5});
        NativeBridge.queuePush(q,new byte[]{6},0,90,90,false);
        check(NativeBridge.queuePoll(q,0)==null,"Wait for keyframe");
        NativeBridge.queuePush(q,new byte[]{7},1,90,270,false);
        CameraEngine.Packet p=NativeBridge.queuePoll(q,0);
        check(p.rotation==180 && Arrays.equals(p.data,new byte[]{4,5,7}),"Config and reverse landscape");
        for (int i=0;i<4;i++) NativeBridge.queuePush(q,new byte[]{8},0,90,90,false);
        check(NativeBridge.queuePush(q,new byte[]{9},0,90,90,false),"Request keyframe on overflow");
        check(NativeBridge.queuePoll(q,0)==null,"Discard old frame backlog");
        NativeBridge.queueReset(q);
        NativeBridge.queuePush(q,new byte[]{10},1,270,90,true);
        check(NativeBridge.queuePoll(q,0).rotation==0,"Front camera orientation");
        check(NativeBridge.queueEncoded(q)==1 && NativeBridge.queueEncoded(q)==0,"Atomic FPS reset");
        NativeBridge.queueDestroy(q);
        check(NativeBridge.queuePoll(q,0)==null,"Service teardown tolerates in-flight poll");
        CameraService service=new CameraService();
        NativeBridge.run(service);
        check(service.connects==2 && service.failures==1 && service.disconnects==2 && service.writes==2,"Reconnect and framed streaming callbacks");
        System.out.println("JNI streaming, exception recovery, rotation, queue and lifecycle checks passed");
    }
}
