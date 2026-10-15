import {useEffect} from 'react';

interface MessageToastProps {
    message: string;
    type: 'success' | 'error';
    show: boolean;
    setShow: (show: boolean) => void;
}

export function MessageToast({ message, type, show, setShow }: MessageToastProps) {
    
    useEffect(() => {
        const timer = setTimeout(() => {
            setShow(false);
        }, 3000);
        
        return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- setShow is a prop; the 3s auto-hide timer must start once on mount, not restart on every parent render
    }, []); 
    
    return (
        show && (
            <span className={`${type === 'success' ? 'text-green-500' : 'text-red-500'}`}>{message}</span>
        )
    );
}