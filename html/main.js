var router ;
var app ;
let wd = new WikiData() ;
var user ;



function load_user_data(callback) {
    $.get("/auth/info",function(d){
        user = d.user;
        if ( user===null ) {
            $("#user_greeting").text("");
            $("#login_logout").html("<span tt='logout'></span>").attr("href","/auth/login");
        } else {
            $("#user_greeting").text(user.username);
            $("#login_logout").html("<span tt='logout'></span>").attr("href","/auth/logout");
        }
        //tt.updateInterface($('#login_logout')) ;
        callback();
    },"json");
}

$(document).ready(function(){
    vue_components.toolname = 'gulp' ;
    Promise.all ( [
            vue_components.loadComponents ( ['tool-translate','wd-link',//'autodesc','wikidatamap','wd-date','tool-navbar','commons-thumbnail',
                'vue_components/main_page.html',
                ] ) ,
            new Promise(function(resolve, reject) { load_user_data ( resolve ) } )
    ] ) .then ( () => {
        current_language = tt.language ;
        wd_link_wd = wd ;
        tt.addILdropdown ( '#tooltranslate_wrapper' ) ;

        const routes = [
          { path: '/', component: MainPage },
/*
          { path: '/group', component: CatalogGroup , props:true },
          { path: '/group/:key', component: CatalogGroup , props:true },
*/
        ] ;

        router = new VueRouter({routes}) ;
        app = new Vue ( { router } ) .$mount('#app') ;
        setTimeout(function(){
            tt.updateInterface($('body')) ;
        },100);
    } ) ;

} ) ;
